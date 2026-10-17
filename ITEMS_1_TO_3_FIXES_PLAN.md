# The fixes for the costs items 1 to 3 added

The ledger (`PROFILE_DRILL_PIWIGO_LEDGER.md`) attributes in its §1 the off-mode instructions
(Ir) that items 1 to 3 of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`) added. This plan fixes
what that attribution found, and the older costs and serial stretches of the load that Step 0,
its measurement of wall time, found on the way. Its rows are the ledger's LOAD-8, LOAD-3.3 and
CACHE-12, and `WORK_STACK.md`'s level 9 tracks it.

## State (2026-10-17)

- **Step 0 is done.** Its results are in "Measurements" below; the ledger's MEAS-4 sums them
  up.
- **The decisions are ruled.** All seven were taken as recommended, and decision 7 was
  confirmed ("Decisions", below).
- **The rows are being built,** since the user's second "go" (2026-10-17), in the order
  below, one commit each, with a pathspec, since another session shares the checkout. R1
  (`6016499f`), R11 (`a5a4d85f`) and R12 (`6705eedd`) are in, and their measurements are on
  the ledger's rows (LOAD-8.1, LOAD-3.3, CACHE-12):
  - R1: −381.2 M off, with no wall change beyond the noise.
  - R11: −0.22 s of the off run's 1.98 s and −214 MiB of peak RSS, most of it the kernel's
    page zeroing, since the parse reuses the freed trees' memory.
  - R12: −54.3 ms of the warm run's 1.39 s (p = 0.009). The decodes moved from the main
    thread into the merge's bucketing on the pool, so the warm Ir fell by only 31.8 M.
  - R15 (`d6ca3442`): −21.6 ms of the off run's 1.83 s (60 pairs, p = 0.031), for +18.9 MiB
    of peak RSS. After R11 the purges are fewer (745, 105.5 MiB, against 987, 154.8 MiB), so
    Step 0's 5.4% became about 1%. The batch commands' public function is `unsafe`, where
    the row said safe (its Change).
  - R2 (`68af1c72`, `276ed5ad`): −18.5 M off, with no wall change beyond the noise. The
    path carries its parent's hash too (its Built).

  R3 is next.
- **Builds and measurement runs need no approval** (since 2026-10-17). Heavy jobs run one at
  a time.
- **Each file and line cited here is as read at `aa896697`,** the code before R1. The rows
  built since move some of them: R1 changes `session.rs`, `project.rs`, `lsp.rs` and
  `source.rs`, R11 `session.rs` and `project.rs`, R12 `name_table.rs`, `members.rs`,
  `class_info.rs`, `function_sig.rs` and `provenance.rs`, R15 `allocator.rs`, `lib.rs` and
  `main.rs`, and R2 `walk.rs`, `session.rs`, `file_table.rs`, `project.rs`, `lsp.rs`,
  `dir_listings.rs` and `vendor.rs`, with `PathHash` and `path_hash` now in the composer
  crate's `hashed_path.rs`.
- **This file and `scripts/` stand alone.** The profiles and binaries it was read from (under
  `~/.cache/prs-profile/`) are not kept: every number it uses is written here, and every
  script it uses is in `scripts/` at the repository root.

## How the rows are designed and judged

- **The most proper fix, not the smallest.** Churn is not a concern. A value is computed
  once, where it is known, and carried; it is never recomputed from a coarser form.
- **An Ir figure is a ceiling, not a saving.** Each one is inclusive or self Ir from the off
  and warm callgrind runs of `aa896697` ("Callgrind and heap figures", below).
- **Ir is not time.** The code runs at 1 G to over 10 G Ir per CPU second, depending on the
  function: allocation, copies and the lexer's name work at 1 to 5 G, and the byte loops most
  rows target (UTF-8 validation, path hashing, component walks) at 10 G or more ("Ir is not
  time", below). Nor does Ir order the rows by wall time: R1's 394 M is spread over 8 threads
  (about 49 M each), while R4's 27 M is all on the main thread, and R6's work (the retain
  closure 112.7 M, `Resolver::path` about 98 M, of thread 11's 393 M) all on the discovery
  thread.
- **Two kinds of row.** An Ir row (R1 to R9, R13) cuts instructions and saves a few ms of wall
  time at most. R1 to R8 cut the off run's Ir, the 20 G plan's total (the ledger measures in
  off mode); R13 cuts the warm run's, and R9 the editor's. An Ir row is called an Ir saving,
  with its wall time shown not to regress beyond the noise. A wall row (R10, R11, R12, R14,
  R15) shortens a stretch where the cores sit unused; it is called a saving on that stretch
  and on the hyperfine medians, never on Ir alone.
- **No saving without a measurement, and no diagnostic changes.** The Piwigo output stays
  byte-identical ("Verification").

## What the attribution found

| cost | at `aa896697` | where it goes |
|---|---:|---|
| a lossy UTF-8 decode of every file (`845efaab`, LOAD-3.4) | 394 M, on 8 pool threads | R1 |
| path handling (`aab67287`, `7bef998a`, and older code) | 481 M of self cost over all `std::path` and `walk::path_hash` symbols: `Path::hash` 194 M (277,176 calls), `Components` iteration about 199 M, `path_hash` 21 M, the rest about 67 M. Not all of it is on the load path | R2 to R8 |
| `canonicalize` in `walk::resolve` | 1.7 M | gone at `aa896697` (the walk fixes after item 1) |
| the content hash of every file (`3334ebc9`, LOAD-1.4) | none in the profile | gone from the batch run (`8ffd68d9`: only with `retain_source` on); the editor's is R9 |
| item 5's +42.4 M (`d870ddcb` to `aa896697`) | diffuse | the sharded merge's own cost (`RefKey` hashing +5.7 M net, rehash +3.6 M, entries under 6 M); its one defect, the `Path::hash` per file (+13.7 M), is R5 |

Where the rows come from (decision 6):

- R1 to R4, R6, R7 and R9 are costs that items 1 to 3 added; R2 to R4, R6 and R7 also take
  in older path code.
- R5 is item 5's.
- R10 is item 1's regression: LOAD-1.2 (`7bef998a`) made every walk wait for `vendor.get()`.
- R8 and R11 to R15 are older costs. R11 is item 3's open LOAD-3.3, R14 completes item 1's
  LOAD-1.1, and R8, R12, R13 and R15 come from no item.

## The rows, in the order they are built

| | row | ledger | what | kind | cost at `aa896697` | needs |
|---:|---|---|---|---|---|---|
| 1 | R1 | LOAD-8.1 | UTF-8 validated once, with the fast check | Ir | 394 M off, 133.6 M warm, on the pool | — |
| 2 | R11 | LOAD-3.3 | context trees dropped on the worker that parsed them | wall, off | 265.2 M; 72 ms on the main thread | — |
| 3 | R12 | CACHE-12 | a file's number bound without decoding its signatures | wall, warm | 393.1 M; 110 ms on the main thread | — |
| 4 | R15 | LOAD-8.15 | a batch run does not purge | wall | 5.4% of the off run's median | — |
| 5 | R2 | LOAD-8.2 | a path that carries its hash | Ir; R3, R4, R6, R7 and R14 use its type | `Tree::node` 24.4 M | — |
| 6 | R3 | LOAD-8.3 | a directory's reach carried down the walk | Ir | 32.4 M + 7.1 M + part of 11.9 M | R2 |
| 7 | R4 | LOAD-8.4 | the walk's hand-over keyed by the carried hash | Ir | 27.3 M; 2 to 8 ms on the main thread | R2 |
| 8 | R6 | LOAD-8.6 | vendor discovery stops hashing paths and stat-ing files | Ir; wall, warm, until R10 | about 49 M of hashing; the retain closure 112.7 M; `Resolver::path` about 98 M | R2 |
| 9 | R14 | LOAD-8.14 | the walk takes discovery's resolved paths | wall until R10 | 14 to 16 ms on the main thread | R2, R6 |
| 10 | R13 | LOAD-8.13 | the autoload files' parse not repeated | Ir, warm; wall, warm, until R10 | 163.0 M; 16 ms warm | — |
| 11 | R10 | LOAD-8.10 | the walk does not wait for vendor discovery | wall, warm | 120 ms with the pool idle | R3 |
| 12 | R5 | LOAD-8.5 | per-file tables keyed by `FileId` | Ir | about 27 M | — |
| 13 | R8 | LOAD-8.8 | identity is a `FileId` inside the scope that owns the table | Ir | about 34 M | R5 |
| 14 | R9 | LOAD-8.9 | the bytes hashed once in the editor | the editor's | 24.1 M per warm load, with `retain_source` on | — |
| 15 | R7 | LOAD-8.7 | the remaining component walks on the load path | Ir | about 61 M to remove | R2 to R6, R8 |

Why this order (decision 4):

- The contained rows come first, with R11 before R15, since R11 changes what is freed and
  when.
- R2 comes before the rows that use its type (R3, R4, R6, R7, R14).
- R6, R13 and R14 come before R10, which hides their wall time. R10 comes after R3, since its
  hand-over rule changes R3's carried reach state.
- The `FileId` rows (R5, R8) go together.
- R7 is last, read from a fresh profile, since R2 to R6 and R8 remove part of what it lists.

**Where wall time can be gained.** The target is the capacity the run leaves unused: 0.75 s of
the 2.02 s off run and 0.85 s of the 1.70 s warm run, at 8 threads ("Where the 8 cores go
unused"). The finishing passes hold the most of it (about 0.26 s off, 0.28 s warm), and they
are REG-7's (decision 1). The rows reach the rest:

- warm: R10, up to about 0.13 s (8% of the run); R12, up to about 0.10 s; R6 and R13, up to
  about 65 ms of the same wait together, and R14 about 16 ms of the stretch right after it,
  only while R10 is not in, because R10 runs both stretches beside the analysed files' parse;
  R4, 2 to 8 ms.
- off: R15, about 5% of the run; R11, about 60 ms; R10, at most 0.07 s, since the stub build
  keeps the pool busy during the wait; R14, up to about 14 ms while R10 is not in; R1, about
  7 ms; R4, 2 to 8 ms.
- R2, R3, R5, R7 and R8 save a few ms each. R9 changes nothing in a batch run; it is the
  editor's.

## Decisions (ruled on 2026-10-17)

Each was taken as recommended, and decision 7 was confirmed.

1. **The finishing passes' unused cores (about 0.27 s) go in REG-7's record, with no row
   here.** REG-7's per-class resolution replaces these passes (`flatten_phpdoc_inheritance`,
   `precompute_ancestor_names`, `precompute_member_owners`,
   `drop_doc_members_excluded_by_native`, `resolve_deferred_value_of`, `flatten_traits`; the
   ledger's REG-7 targets), so a row here would parallelize code that REG-7 deletes.
   - The rest of `run_finishing_passes` (`project.rs:1507`) has owners too. REG-7.2 to 7.4
     take the `@method` tags, the imported aliases, the assertion step, the property hooks,
     the mixins and the constant-named types. REG-4.2 (READY) deletes
     `canonicalize_class_name_case`. Only `infer_global_variables_of` stays: it is the one
     whole-program pass REG-7.8 keeps.
   - The cost stays until REG-7, which waits on REG-16.3. The ledger's REG-7 notes hold the
     numbers.
2. **The batch run stops purging (R15), and the huge pages stay.** Purging costs a batch run
   about 5% of its wall time and every TLB-shootdown IPI, and gains it nothing. Without huge
   pages the run takes 3.6 to 4.1 s, against 2.7 to 2.9 s. The page zeroing is the first touch
   of the footprint, which only a smaller footprint lowers (R11). Measured once R11 was in
   (R15's Built): purging costs about 1% of the off run, and it lowers the peak RSS by 19 MiB.
3. **R14 is its own row, after R2 and R6 and before R10.** It changes the walk's entry point (a
   `Resolved` path that is not resolved again), not discovery, and it builds on R2's hashed
   path and on the listing kinds R6 reads. Its 14 to 16 ms can be measured only before R10
   hides them.
4. **The order is R1; R11, R12, R15; R2, R3, R4, R6, R14, R13; R10; R5, R8; R9; R7:** the
   table's above, for the reasons under it.
5. **The stub-base wait (89 ms, cold stub cache only) is the STUB rows'.** STUB-11 (no run
   builds the builtin knowledge, and the stub disk cache goes) removes what a cold run waits
   for. STUB-13.2 (`Project::loading` goes, and with it `expect_files`' choice of base)
   removes the wait. The 89 ms and the cold stub build (0 to 0.21 s off, at 4.7 cores) are in
   the ledger's STUB notes, for STUB-10.1's "what a cold run still pays".
6. **The rows whose costs predate items 1 to 3 (R8, R11 to R15) stay.** R11 is item 3's own
   open row (LOAD-3.3), and R14 completes item 1's LOAD-1.1 ("each link is resolved once"),
   so both are items 1 to 3's unfinished work. R8, R12, R13 and R15 sit on the measured
   critical path or in the Ir total; R12's decode was already `insert_built_decl`'s 413.0 M
   at the base. R10 is not among these: it is item 1's own regression.
7. **LOAD-2.2's name floor (item 2).** The editor's three uses of the floor skip Composer's
   vendor directory by its resolved path, so a `vendor/` outside Composer is walked, and keep
   the `.git` and `node_modules` names until `exclude_paths` (the ledger's LOAD-7.8) exists;
   then those names go, and with them `path_under_excluded_dir` and
   `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`. This combines item 2's options a and b. The work is item
   2's, not this plan's; the ledger's LOAD-2.2 and `WORK_STACK.md`'s item 2 record it.

## Rows

Each row gives what it costs at `aa896697`, what the code does now, the change, its test, how
it is measured, and what the ledger records when it lands. The files named without a
directory are: `session.rs`, `walk.rs`, `file_table.rs`, `use_sites.rs`, `source.rs`,
`file_cache.rs`, `class_shape.rs` and `composer_hygiene.rs` in `crates/phpstan-rs/src/project/`;
`project.rs`, `lsp.rs`, `allocator.rs` and `lib.rs` in `crates/phpstan-rs/src/`; `index.rs`,
`name_table.rs`, `class_info.rs`, `provenance.rs` and `resolve.rs` in
`crates/phpstan-rs/src/registry/`; and `vendor.rs` and `dir_listings.rs` in
`crates/composer/src/`. mimalloc's C sources (`options.c`, `os.c`, `prim.c`, `arena.c`,
`mimalloc.h`) are those of `libmimalloc-sys` 0.1.49, under `c_src/mimalloc/v3/`.

### 1. R1: UTF-8 validated once, with the fast check

*LOAD-8.1. An Ir row, off and warm. Needs nothing.*

**Cost.** 394 M off, on the 8 pool threads; 133.6 M warm, where the analysed files are parsed
again because they are never cached. Time: 21 perf samples off (42 ms of CPU over the pool,
about 7 ms of wall at the parse's 6 cores) and 11 warm (22 ms).

**Now.** `read_and_parse_file` (`session.rs:1224`) runs `String::from_utf8_lossy(bytes)` over
every file to take its length (LOAD-3.4, `845efaab`), and its `Utf8Chunks::next` validates byte
by byte. The same lossy decode, with an owned copy, sits at three more sites, which the editor
reaches when `retain_source` is on: `session.rs:1213` (a cache hit's kept source),
`project.rs:2049` (`update_file`'s kept source) and `lsp.rs:1290`.

**Change.**

- One helper, `decode_text(bytes) -> Cow<str>`, validates with `simdutf8` and decodes lossily
  only on failure. It serves all four sites; the owned copies become a plain copy of the
  validated `&str`.
- In `read_and_parse_file`:
  `match simdutf8::basic::from_utf8(bytes) { Ok(t) => (Cow::Borrowed(t), t.len()), Err(_) => lossy Owned }`;
  the `Cow` arms at `:1234-1252` stay.
- Checking validity first, with the lossy reading only on failure, gives the same `len`, `text`
  and parse for valid and invalid files, so the length of a non-UTF-8 file needs no ruling.
- **Why `simdutf8`.** Q-9 (`276c9d7d`) puts its check at a third of `std`'s, so the check costs
  about 11 M with it and about 33 M with `std::str::from_utf8`, against the lossy decode's
  394 M. Both remove most of the cost; `simdutf8` takes the other 22 M for a new dependency.
  Its `&str` is borrowed, so the copy that made LOAD-6 a SKIP does not arise: LOAD-6 measured
  a `simdutf8` run at 23,215,574,656 Ir against 23,216,039,715 (−0.47 M where about −22 M was
  estimated) and was reverted for its `to_owned` copy.
- **The dependency, judged on merit.** `simdutf8` 0.1.5 (in the cargo registry from the LOAD-6
  build) has `basic::from_utf8(&[u8]) -> Result<&str, Utf8Error>`, borrowed, and is licensed
  MIT OR Apache-2.0. Add it to `crates/phpstan-rs/Cargo.toml`; `Cargo.lock` changes. CI runs
  `cargo deny check` (`rust-ci.yml:111`; `deny.toml` allows MIT and Apache-2.0) and
  `cargo-machete`, so run both locally if installed. Our code stays free of `unsafe`
  (`unsafe_code = "deny"`); the crate's own `unsafe` is a dependency's.

**Test.** The existing
`a_file_that_is_not_utf8_is_parsed_from_its_bytes_and_weighed_by_its_lossy_length`
(`project/tests/load_text.rs:77`) pins the invalid case. Add the valid counterpart (the length
equals the byte length, and the file is parsed from the text), and a differential test of
`simdutf8::basic::from_utf8` against `std::str::from_utf8` on valid, truncated and overlong
inputs.

**Measure.** Off and warm callgrind: the inclusive Ir under `read_indexed`.

**Ledger, when it lands.** Correct LOAD-3.4's note (`4c425d14`), which says the fix needs a
ruling on the length, and LOAD-6's record, whose SKIP was for the owned `String`.

### 2. R11: context trees dropped on the worker that parsed them

*LOAD-3.3, for the load. A wall row, off. Needs nothing.*

**Cost.** 265.2 M, serial on the main thread in the off run: `drop_glue<[Stmt]>`
(`Arc<Vec<Stmt>>::drop_slow` 121.5 M, `StmtKind` 111.2 M), the main thread's largest cost in
`Session::load` (424.1 M) and nearly half of thread 01's 538.7 M. None in the warm run, whose
cache hits carry no tree. Time: `insert_numbered_file` is 41 main-thread samples off (82 ms,
1.080 to 1.162 s), 36 of them freeing the trees (72 ms: `drop_glue::<[Stmt]>`, with `mi_free`
and `mi_free_try_collect_mt`, mimalloc's path for a block another thread allocated, under it).
It is on the critical path: the finishing passes start only after numbering.

**Now.** `Project::keeps_tree` documents that a built-once project "drops each context tree on
the thread that parsed it", but nothing does: `read_and_parse_file` returns the `ParsedFile`
with its `ast`, and `insert_numbered_file` (`project.rs:1131`) runs `ast.filter(|_| keeps_tree)`
on the main thread. Every context tree stays alive from its parse until the main thread
reaches it, which raises the heap peak.

**Change.** In `read_indexed`, after the cache entry is stored, a file for which
`!project.keeps_tree(analysed)` sets `parsed.ast = None`. The readers downstream already
tolerate a missing tree: a cache hit has none, and `insert_numbered_file`'s debug assertion
allows `ast: None` for a context file of a built-once project.

The ledger's LOAD-3.3 waits on LOAD-3.1: a tree is kept by one function of (role, open), in
the load and in `update_file` alike. For the load that holds already:

- in a built-once project `keeps_tree` is a function of the role alone (`project.rs:953`);
- LOAD-2.2's walk makes each file's role final before its parse.

So this row is LOAD-3.3 for the load, and `update_file`, which keeps every tree, stays
LOAD-3.1's.

**Gain.**

- The drop moves to the pool, so the Ir total stays and the main thread falls by about 265 M.
  The frees run inside the parse, at about 6 cores (about 12 ms there), and become
  thread-local: about 60 ms off, nothing warm.
- The heap peak falls (dhat, one thread, off; the peak at 13.0 s of a 25.6 s run). The
  allocations that `syntax::parse::parse_file` makes under `read_indexed` hold 427.6 MB at the
  peak and 185.5 MB at exit, where only the analysed files' trees are left, so the context
  trees are at least 242.1 MB of the 809.6 MB peak (30%). Freed during the parse, they leave
  about 568 MB at the old peak's moment, and the new peak is what is alive later; the exit
  holds 641.6 MB, so the peak falls by at most 168 MB (21%).
- By stack, the peak is `read_indexed` only 454.1 MB (56.1%), both `read_indexed` and
  `Walk::walk_dir` 216.5 MB (26.7%: parse allocations under `walk_dir`'s frames, since
  `dirs.par_iter()` runs parse tasks while it waits for its subdirectories), `walk_dir` only
  1.8 MB (0.2%), and neither 137.2 MB (16.9%).
- A smaller footprint also lowers the page zeroing, the largest single leaf in the kernel
  ("Kernel time").

**Test.** A built-once load keeps no context tree (`kept_ast` is `None` for context files), and
an analysed file keeps its own; the Piwigo oracle.

**Measure.** Off callgrind (the main thread's Ir), and the off run's stretch from the parse's
end to the finishing passes in a perf recording, with hyperfine medians.

**Ledger, when it lands.** LOAD-3.3's record says that `update_file` waits on LOAD-3.1.

### 3. R12: a file's number bound without decoding its signatures

*CACHE-12. A wall row, warm. Needs nothing.*

**Cost.** 393.1 M, serial on the main thread in the warm run: `OnceLock::initialize`, 393.1 M of
`bind_file`'s 401.0 M and 60% of thread 01's warm 651.7 M. None in the off run, whose
declarations are built decoded. Time: 55 main-thread samples warm (110 ms, 0.592 to 0.702 s)
with the pool idle; 5 off. The warm run decodes 655.9 M in all (`Once::call`); the later
passes decode 177.8 M (`resolve_deferred_value_of`) and 87.9 M (`Members::eq`) more.

**Now.** `ParsedFile::bind_file` (`project.rs:3259`) gives every pending declaration its file's
number with `Arc::make_mut(sig).bind_file(file)` (`index.rs:53`, `class_info.rs:958`). `make_mut`
on an `Arc<Lazy<FunctionReflection>>` calls `Lazy::get_mut` (`name_table.rs:528`), which decodes
the postcard entry, for every method of every cached class (through `Members`'
`Arc<Lazy<FunctionReflection>>`), to write one field that the encoded form does not hold. It
must decode: `Provenance` encodes `Origin::File(_)` and `Origin::PendingFile` alike as
`OriginData::File`, which decodes to `PendingFile` (`provenance.rs:110`, `:135`), so a decoded
entry is pending until bound.

**Change** (read in the code on 2026-10-17, and again when the row was written).

- **The number is bound without decoding.** `Lazy` keeps the file's number beside the encoded
  bytes, and its `bind_file` sets it without decoding; `get` binds the value it decodes before
  it hands it out, so no reader sees a pending entry. This is CACHE-12's decode context, for a
  `FileId` where CACHE-12 had an `Arc<Path>`, so CACHE-12 no longer waits on CACHE-11's
  (CACHE-11 waits on CACHE-9 and CACHE-10, which are BLOCKED). `Members::bind_file` binds each
  member so, and `ClassReflection::bind_file` calls it where it walks `iter_mut` today.
- **The bytes stay while they describe the value.** `Lazy::get_mut` drops them, since a write
  may change the value; a read keeps them, and so does a binding, which the bytes and the
  number describe together. Two values that both still hold their bytes are equal when their
  bytes and numbers are, with no decode; otherwise both are decoded and compared, as now. A
  value stored again writes its bytes while it holds them, decoded or not.
- **That is for the third decoder, `changed_since`'s comparison.** `Members::eq` (87.9 M warm,
  95.7 M inclusive) is called from code inlined into `run_finishing_passes`, right after
  `drop_doc_members_excluded_by_native` (`project.rs:1568`): `changed_since`'s `differs`
  (`registry.rs:204`) compares each class a pass rewrote with the form that
  `end_declaration_changes` took out before the passes, through `ClassReflection`'s derived
  `PartialEq` (its `methods` field, `class_info.rs:379`). A pass that writes a class's methods
  through `iter_mut`, `values_mut` or `retain` copies each shared member (`Arc::make_mut`), so
  the two sides hold distinct `Lazy` values, and the comparison decodes the side taken out.
  The warm run's project registry holds resolved declarations (`reset_scoped` 34.2 M for the
  classes, 12.8 M for the functions), so the comparison runs there, serially, on the finishing
  passes' thread. Traced on 2026-10-17 (`callers.py` on
  `OnceLock<…FunctionReflection>::initialize`, warm): the decodes by caller are
  `ParsedFile::bind_file` 393.4 M, the deferred-type scan 177.8 M, `Members::eq` 87.9 M,
  `flatten_traits_one_pass` 3.0 M and `flatten_phpdoc_inheritance_one_pass` 0.2 M. With the
  bytes kept, a member no pass wrote compares by its bytes, the as-built form's against the
  resolved form's, which are the same bytes for a method the passes left as it was; only a
  member a pass wrote, or one whose resolved form differs, is decoded.
- **No deferred-type fact.** This row was to carry, in each encoded entry, whether it holds a
  deferred type, for `class_has_deferred_type`'s scan (`resolve.rs:592`, `:1486`) to read
  instead of decoding. Read in the code, the fact would save nothing. In a fresh load every
  name a project file declares is in the passes' scope, and three later passes read every
  method of every class in it: `flatten_phpdoc_inheritance_one_pass` (`resolve.rs:1162`, each
  method's `has_own_doc`), `resolve_constant_named_types` (`:1946`) and
  `drop_doc_members_excluded_by_native` (`:1759`). So the scan's decodes would only move to the
  first of them, as CACHE-7's 420 M moved to the workers. Those reads are REG-7's to remove
  (decision 1), and the scan goes with REG-7.2.
- **Where the decode lands.** Before the passes, the declaration merge's `bucket`
  (`declaration_merge.rs:235`, run on the pool by `merge_by_shard` in 32 chunks) reads every
  method of every class it merges for the class's assert-tagged names
  (`for_each_assert_tagged_name`, `index.rs:570`). Once `bind_file` does not decode, that is
  the first reader, so the decode moves from the serial main thread to the merge's parallel
  bucketing.
- **The caches' formats stay.** The bytes and their encoding do not change; only what a `Lazy`
  keeps in memory does.
- **Rejected:** running `bind_numbers` (`session.rs:634`, a serial loop over the files) on the
  pool. It spreads the decode over the pool, but it still decodes every method of every cached
  class to write one field.

**Gain.** Up to 393 M off the main thread, and up to about 0.10 s warm, less what the decode
costs where it lands, in the merge's bucketing on the pool. The binding saves no Ir: the merge
decodes what `bind_file` decoded. The comparison's 87.9 M, serial on the finishing passes'
thread, falls by the members compared by their bytes. Both are measured, not assumed.

**Test.** A stored class's members take their file's number without being decoded, and a member
decoded afterwards names its file; two read-backs of the same members are equal with no decode,
a written member is compared by value, and members bound to different files are not equal; the
warm Piwigo oracle.

**Measure.** Warm callgrind: `bind_file`, the decodes by caller, `Members::eq`. The warm run's
`bind_file` stretch, the merge and the finishing passes in a perf recording, against the row
before it; `scripts/abab.py`, warm.

### 4. R15: a batch run does not purge

*LOAD-8.15. A wall row, measured off. Needs nothing; measured after R11.*

**Cost.** With `MIMALLOC_PURGE_DELAY=-1` in the environment, the off run's medians were 2.228
against 2.108 s (−5.4%, p = 0.040), and its TLB-shootdown IPIs went away ("The allocator
runs").

**Why a batch run should not purge.**

- mimalloc v3 purges memory that has been free for `purge_delay` (1000 ms) with
  `MADV_DONTNEED`: 987 purges, 154.8 MiB in all, in an off run on Piwigo.
- Each purge flushes the TLBs of the other threads that map the range
  (`smp_call_function_many_cond`).
- With `allow_thp` at 1, a purge can be as small as 64 KiB (`options.c:178`), and they average
  160 KiB here. A purge smaller than 2 MiB inside a huge page splits that page.
- A batch run gains nothing from purging. The peak is the same with and without it (1.0 GiB
  of RSS), and the memory it hands back is free only until the process exits.

**Who keeps the default.** The language server lives long and should give memory back after a
large analysis, so it keeps the default. The batch commands (`check`, `check-project`, `demo`
and `convert-baseline`) set `purge_delay` to −1 at the start of `main`'s dispatch, before the
worker pool exists. `lsp` and `incremental-bench`, which measures the editor's path, keep the
default. These are all six of `main.rs`'s commands.

**Change.**

- **Not through the environment.** mimalloc reads every option from the environment once, when
  the process loads (`_mi_options_init`, `options.c:191-196`), so `main` cannot set it that
  way.
- **`mi_option_set`.** It writes the option (`libmimalloc-sys` `extended.rs:617`;
  `options.c:298-304`). Purging reads the option each time it schedules a purge (`os.c:642`,
  `arena.c:2022`), so a set after load takes effect. The binding declares the options API
  under its `extended` feature, which adds the `cty` crate (C type names, MIT OR Apache-2.0)
  and changes nothing in the C build.
- **The option's index.** The binding defines no constant for v3's `mi_option_purge_delay`.
  It is index 15 of the enum in `include/mimalloc.h`: the binding's 14
  (`mi_option_eager_commit_delay`) and 16 (`mi_option_use_numa_nodes`) bracket it. So the row
  declares it beside the call.
- **Where the `unsafe` lives.** The call is `unsafe`, since it is an `extern "C"` function. It
  sits in `allocator.rs`, the module that already reaches mimalloc's C API under
  `#[allow(unsafe_code)]` (`lib.rs:24-26`).
  - **The public function is `unsafe` too,** where this row first said safe. `mi_option_set`
    writes the option with no synchronization, and the allocator reads it as it purges, so a
    safe function would let a caller set it while other threads run.
    `phpstan_rs::keep_freed_memory` states the contract, that no other thread is running, and
    `main` meets it: it calls the function before any thread starts.
  - **Not with the `hotpath` feature.** Its guard starts threads before `main`'s body runs, so
    such a build keeps the default.
- **Other allocators.** With the `dhat-heap`, `hotpath-alloc` or `system-allocator` features
  that module is not compiled, and the function does nothing.
- **Not this row:** `allow_thp`, since the huge pages stay (decision 2); and the editor's purge
  settings. Whether the editor's purges should be whole huge pages (`allow_thp` at 2 makes the
  smallest purge 2 MiB) is a question for its own workload.

**Test.** The option reads 1000 by default. The version is pinned (`=0.1.49`), so a bump runs
that test again. The other half, −1 after the call, is not a test: a test binary runs other
threads, so the call's contract cannot be met there. mimalloc's exit statistics show the
effect instead: no purge.

**Measure.** After R11, which changes what is freed and when: the hyperfine set of "The
allocator runs" again, alternating A B A B, with the compaction counters; one perf recording,
for the IPI count; the Piwigo oracle.

**Built** as `d6ca3442`. After R11 purging costs about 1% of the off run, not 5.4%, and it
lowers the peak RSS by 19 MiB, which Step 0's statistics (1.0 GiB in both) could not resolve.
The ledger's LOAD-8.15 has the numbers.

### 5. R2: a path that carries its hash

*LOAD-8.2. An Ir row, and the type that R3, R4, R6, R7 and R14 use. Needs nothing.*

**Cost.** `Tree::node(dir)` (`walk.rs:408`, called by `attach` for every placed file) computes
`path_hash(dir)` from the directory's components: 20.9 M, plus 3.5 M for its recursion, in the
off run, on the serial merge (`Walk::merge` 50.5 M inclusive, thread 02), though `walk_dir`
knew the directory's hash when it listed the file (Table 3). Time: `Starts::reaching`,
`Walk::merge`, `Tree::node`, `walk::path_hash` and `prunes`, the code of R2, R3 and R7, are at
most 17 samples off and 18 warm, overlapping.

**Change.**

- Move `PathHash`, `path_hash` and `child_hash` from `phpstan-rs/src/project/walk.rs` into the
  `composer` crate (`phpstan-rs` depends on it).
- A newtype, `HashedPath { path, hash }`, keys `Listed`, `Found::File`, `DirListings` and the
  walk's tables, which are looked up by it.
- A hash is computed in one place, the constructor `HashedPath::new(path)`, and extended by
  `HashedPath::child(name)`. The constructor stays public, since roots, configured paths,
  manifests and the editor's paths enter through it. So the type does not make a load path
  unable to recompute a hash; it makes a recomputation an explicit `HashedPath::new` call that
  a grep, and a review, find.
- `FileTable::id_of(&Path)` stays as the editor's entry point.
- `Found::File` carries its directory's `HashedPath`, and `attach` takes it.

**Test.** No behaviour changes; the `walk_identity.rs` and `project::` tests are its check.

**Measure.** Off callgrind: `Tree::node` and `Walk::merge`.

**Built** as `68af1c72` and `276ed5ad`: −18.5 M off. A `HashedPath` carries its parent's hash
too, so a listed file names its directory's hash, and `Found::File` carries only the file's
path, not its directory's `HashedPath` (a copy of the directory's path per file). The tree
finds a new directory's nodes with `hashed_ancestors`, one fold of the hashes and then the
ancestors from the parent up; the first commit made every ancestor (11.0 M), the second only
those it reaches (3.0 M). R3 and R6 use `hashed_ancestors` too. The ledger's LOAD-8.2 has the
numbers.

### 6. R3: a directory's reach carried down the walk

*LOAD-8.3. An Ir row. Needs R2.*

**Cost.** On the pool: `Starts::reaching`'s path hashes 32.4 M, `prunes`' 7.1 M and part of
`walk_dir`'s 11.9 M (8.0 M + 3.9 M). `Starts::of` hashes 2.0 M, plus 2.1 M to rehash its
`PathBuf` tables. `Starts::reaching` is 74.9 M inclusive over the pool threads (Table 7), and
its component walks 30.1 M, `prunes`' 5.0 M (Table 4).

**Now.** `Starts::reaching` (`walk.rs:257`) scans every ancestor of every directory with two
path lookups each (`covering.contains`, `self.dirs.get`): quadratic in depth. `prunes` (`:834`)
does the same per child directory, and `walk_dir` calls
`self.starts.files.contains_key(&child)` per PHP file in a sole-reach directory (`:764`).

**Change.**

- Each directory task receives `{ walks reaching it, covered }`. A child's state is the
  parent's, minus the walks whose `excluded` names the child, plus the walks that start
  exactly at the child, found by its carried hash.
- `Starts.dirs`, `Starts.files` and `covering` become `hashbrown::HashTable`s keyed by
  `PathHash` with path equality, as `FileTable` (`ids: HashTable<FileId>`) and `FileIndex`
  (`walk.rs:105`) already are.
- A walk's own state at its root comes from one ancestor scan of the old `reaching`: a walk W1
  entered below another walk W2's root still counts W2 as reaching, and W2's exclusions are
  checked from W2's root down. Below the root the state is carried.

**Test.** Keep the old `reaching` as a test-only oracle, and assert equality on the
`walk_identity.rs` cases (nested roots, excluded names, covering roots, links, reversed roots,
the unplaceable fallback).

**Measure.** Off callgrind: `Starts::reaching`, `prunes` and `walk_dir`.

### 7. R4: the walk's hand-over keyed by the carried hash

*LOAD-8.4. An Ir row. Needs R2.*

**Cost.** 27.3 M on the main thread: 28,502 path hashes, 2 for each of the 14,251 files. It sits
on the critical path, after the listing and before the deferred parse and numbering, but it is
small: from the parse's end to `bind_file` the main thread has 4 samples (8 ms) in each mode,
of which `Path::hash` is 1 off and 2 warm, so 2 to 8 ms.

**Now.** `StreamParse::into_parsed` (`session.rs:274`) builds an
`FxHashMap<PathBuf, (Role, Read)>`, and `Session::load` (`session.rs:1503-1513`) calls
`streamed.into_parsed()` and removes each file from the map (`parsed.remove(&file.path)`) on the
calling thread. `into_parsed` is inlined into
`Session::load` (callgrind's `Path::hash` edge for it comes from `load` itself), so its samples
are `load`'s own. Every file is handed over: all 14,251 calls to `read_indexed` come from the
walk's `take` closure, none from the deferred parse. `Listed` already carries `hash`; the
parsed tuple drops it.

**Change.** Key the parses by `Listed`'s hash in a `HashTable` with path equality, as
`FileTable` is. Rejected on merit, not size: a dense index assigned by the walk needs a counter
shared by the walk's parallel tasks and a field carried through `Found`, `merge` and `emit`,
which is new state to keep consistent; the carried hash is data the walk already holds and
the table already uses.

**Test.** No behaviour changes; the `walk_identity.rs` and `load_text.rs` tests are its check.

**Measure.** Off callgrind: the main thread's `Path::hash` under `Session::load`.

### 8. R6: vendor discovery stops hashing paths and stat-ing files

*LOAD-8.6. An Ir row, and a warm wall row until R10. Needs R2.*

**Cost.** All on the discovery thread (thread 11 off, 393.3 M; Table 5):

- path hashing, about 49 M: 23.2 M in the `drop_files_under_dirs` retain closure, 16.6 M in
  `listable_dir` and 9.1 M in `DirListings::listing` (the thread's `Path::hash` self cost is
  53.4 M in all); the closure's `keep_first` and `dedup` hash `&Path` sets, 4.8 M plus 2.2 M
  of rehash;
- the closure's component walks, 50.2 M (`strip_prefix` 27.7 M, `parent` 21.0 M); the whole
  retain closure is 112.7 M inclusive;
- `Resolver::path` (`vendor.rs:254`, about 98 M, called for every autoload entry of the
  generated files): `starts_with` 25.2 M, `listed` 30.7 M inclusive, `lexically_normalized`
  11.5 M, `append_concat` 14.2 M, `entry_canonical` 8.4 M.

Time: discovery is 73 samples off and 56 warm on its own thread (0.041 to 0.208 s, and 0.029
to 0.144 s). The retain closure is 22 and 20 of them, mostly kernel (`statx` and
`__d_lookup_rcu`: the per-file `symlink_metadata`), so reading a file's type from the listing
saves more time than the hashing does. `Resolver::path` is 35 and 22, of which `listed` (the
directory reads, which stay) is 28 and 17. In warm mode discovery is on the critical path,
since the pool waits for it, until R10 lands; after R10 this row saves CPU only.

**Now.** `drop_files_under_dirs`, `walked_by_a_directory` and `listable_dir` (`vendor.rs:451-`)
hash a `PathBuf` per ancestor and call `symlink_metadata` per file, and
`DirListings::listing` hashes the directory: its table, `listed`, is a
`Mutex<FxHashMap<PathBuf, Slot>>` (`dir_listings.rs:15`). Per entry, `Resolver::path` does
`resolved.starts_with(self.vendor_dir)`, `canonical`'s `listed(dir)`, `lexically_normalized`,
`append_concat` and `entry_canonical`. `Resolver::listed` caches each directory's listing in
`listings`, keyed by the directory's text as bytes (`vendor.rs:197-203`): the precedent for
avoiding `Path::hash`.

**Change, the drop.**

- `DirListings.listed` is keyed by `PathHash`.
- The files are grouped by parent directory, and each distinct directory's components are
  folded once: every ancestor's hash falls out of that one fold.
- A file's type is read from its parent's `Listing::entry(name)` (`dir_listings.rs:79`), whose
  `kind` is the dirent type, with `symlink_metadata` as its own fallback.
- The drop itself stays: it lets those files parse during the walk (LOAD-2.4).
- The `composer` crate gains a `hashbrown` dependency: it has `rustc-hash` only, and
  `HashTable` is hashbrown's.

**Change, `Resolver::path`** (its callees read one by one on 2026-10-17, `vendor.rs:254-343`).
A path that `canonical` returned is canonical and normalized, so its containment in
`vendor_dir` is a byte-prefix test at a separator boundary: sound only for normalized paths,
which `resolved` is and `raw` is not. The directory's listing is already found once per
distinct directory text.

- **Containment is decided once per listed directory**, not per entry. `Listed` carries where
  its canonical directory lies, in one of three states:
  - in `vendor_dir`: every entry lies in it;
  - `vendor_dir`'s parent: only the entry that is `vendor_dir` itself does;
  - elsewhere: no entry does.

  Only a link is tested itself, since `entry_canonical` resolves it with `fs::canonicalize`
  and it may lead anywhere.
- **The as-written fallback works the same way.** `vendor_as_written`
  (`lexically_normalized(&self.generated_dir.join(".."))`, made again for every fallback entry
  at `:265`) is made once, in `Resolver::new`. A directory's normalized text and its verdict
  are kept beside its listing, so a fallback entry is that text joined with its name.
- **`entry_canonical`'s join stays** (`listed.canonical.join(name)`, `:340`). It is the one
  place a resolved child is made, and it makes R2's `HashedPath` with `child`.
- **`append_concat` stays per entry in off mode** (`:394-426`). R13 caches what it evaluates,
  so a warm run does not run it.

**Gain.** In warm mode, while R10 is not in, R6 and R13 together shorten the vendor wait by up
to about 65 ms: the retain closure's 40 ms, 24 ms of it the per-file `statx`;
`Resolver::path`'s 10 ms outside `listed`; and the autoload parse's 16 ms (R13).

**Test.** The grouped drop equals the old per-file result on a fixture.

**Measure.** Off and warm callgrind on the discovery thread, with `parse_generated_file` (R13's
163 M of its 393 M) counted apart, and the warm vendor wait in a perf recording. The main
thread's second resolution of discovery's paths is R14's.

### 9. R14: the walk takes discovery's resolved paths

*LOAD-8.14. A wall row until R10, then CPU and system calls. Needs R2 and R6.*

**Cost.** 14 to 16 ms, serial on the main thread in both modes, before the walk can start: 7
samples off (0.210 to 0.222 s) and 8 warm (0.147 to 0.161 s; 5 under `Walk::add` and
`add_file`, 3 right under `walk_listing`, which is inlined: a `Path::join`, a `Components::eq`
and a copy). The main thread's kernel samples there are `link_path_walk`, `do_statx`,
`apparmor_inode_getattr` and `__audit_inode`. By the code, most of the 7,493 failing
`readlink`s and about 2,000 of the 29,161 `statx` calls in the off run's strace count are the
main thread's, here; the count is not split by thread ("System calls"). The cost is older than
items 1 to 3, and the row completes item 1's LOAD-1.1, "each link is resolved once".

**Now.**

- `walk_listing` (`session.rs:1083-1091`) joins each directory and file that discovery returns
  to the root (a no-op: they are absolute), tests it with `is_promoted` (which returns at once
  on Piwigo, since it has no path repositories), and hands it to `Walk::add` (249
  directories) or `Walk::add_file` (1,744 files), which resolve it again.
- `Walk::add` (`walk.rs:513`) resolves its root with `composer::manifest::resolve`
  (`walk.rs:36`), a `fs::canonicalize`, and then stats it (`is_dir`). `add_file`'s
  `resolve_file` (`walk.rs:883`) takes a `fs::symlink_metadata` (one `statx`) per file and
  resolves each distinct parent. glibc's `realpath`, behind `fs::canonicalize`, calls
  `readlink` once per path component, and EINVAL means the component is not a link; so each
  root and each distinct parent costs one `readlink` per component, six or more on Piwigo's
  `vendor/` paths.
- Discovery itself resolves a path through its directory's listing (`vendor.rs:270-343`) and
  calls `fs::canonicalize` only for a link, a directory it cannot list, and the vendor
  directory (`vendor.rs:56`). The walk's other calls are one per link it meets (`walk.rs:736`)
  and one per configured root.
- `Resolver::path` (`vendor.rs:254`) returns one of two forms: the canonical path, from
  `canonical` or `entry_canonical`, when it lies in the vendor directory (the common case);
  or, for a path-repository package whose `vendor/<name>` is a link to a directory outside,
  the path as written, `lexically_normalized`, which is not canonical. One more path is not
  resolved at all: `ClassLoader.php`, the project directory joined with `vendor/composer`
  (`vendor.rs:84-85`, after an `is_file` stat).

**Change.**

- `VendorContext` keeps the two forms apart: its `dirs` and `files` each split into resolved
  and as-written.
- `ClassLoader.php` is resolved as the autoload entries are, through `Resolver::canonical`,
  which replaces the stat.
- The walk takes a resolved path through an entry point that does not resolve it again, with
  the kind its listing entry gave (R6 reads it there too) instead of a `statx`.
- The as-written paths, a path discovery resolved through a link (the listing gives the
  link's kind, not its target's) and the configured roots go through today's `add` and
  `add_file`.
- A type, `Resolved(PathBuf)`, keeps an unresolved path out of the new entry point: a path
  that `resolve` maps to itself, built only where that is known (`canonical` and
  `entry_canonical` in the composer crate, and `resolve` itself).
- R7 does not cover this cost: it removes component walks, not system calls.

**Gain.** Like R6's and R13's, wall time only until R10 lands: R10 adds the vendor roots while
the analysed and stub walks run, and R14 then saves the main thread's CPU and the system
calls.

**Test.** A vendor tree with a linked path-repository package and a linked file gives the same
roots, files, roles and hand-overs through the new entry point as through today's; the
Piwigo oracle.

**Measure.** The main thread's stretch from `vendor.get()`'s return to the walk's start in a
perf recording (0.210 to 0.222 s off and 0.147 to 0.161 s warm today), and the main thread's
`readlink` and `statx` counts from `strace -ff` (Appendix D), before and after.

### 10. R13: the autoload files' parse not repeated

*LOAD-8.13. An Ir row in warm runs, and a warm wall row until R10. Needs nothing.*

**Cost.** `parse_generated_file`, 163.0 M in the off and the warm run alike (`parse_program`
137.1 M, `Lexer::build` 25.0 M). Piwigo's `autoload_static.php` is 1,808,580 bytes with 13,711
lines, and one non-ASCII byte in it sends `LineIndex::new` down its whole-file `Utf8Chunks`
path, 12.7 M. It is not from items 1 to 3: thread 11 is 393.31 M at `aa896697` and 393.30 M at
the base. Time: 8 of discovery's 56 samples warm (16 ms of 112 ms) and 15 of 73 off (30 ms),
far less than its 41% of the thread's Ir. The discovery thread is on the critical path in warm
mode, where the pool idles until it ends, and not in off mode, where the stub build runs
beside it.

**Now.** The file is read for its three array properties, and `Resolver` walks only their
entries.

**Change.** Keep the extracted entries, not the parse: read them from the tree once, before
`Resolver`'s path work (which depends on the file system, and so is not cached), and store them
in the per-file cache under the file's content key, as the parsed files are. A warm load then
skips the parse. The off run, which has no cache, still parses; it gains only from a cheaper
`LineIndex`, which SYN-3.1 and SYN-3.2 own.

**Gain.** The 163 M in warm runs only, and about 16 ms of warm wall time while R10 is not in;
none once it is. It comes before R10 (decision 4), so its wall share is measured while it
still has one.

**Test.** A warm load finds the same vendor context as a cold one, on a fixture with a
generated `autoload_static.php`.

**Measure.** Warm callgrind (`parse_generated_file` on the discovery thread), and the warm
vendor wait in a perf recording.

### 11. R10: the walk does not wait for vendor discovery

*LOAD-8.10. A wall row, warm. Needs R3.*

**Cost.** In warm mode the main thread waits 120 ms in `vendor.get()` (0.027 to 0.147 s) with
the pool idle: 0.13 s of unused capacity, the largest stretch any row here reaches (only the
finishing passes, 0.28 s, and the check, 0.17 s, leave more). In off mode the stub build keeps
the pool at 5.5 cores through the wait (0.042 to 0.210 s), so at most 0.07 s is left unused
there. It is item 1's regression: before LOAD-1.2 (`7bef998a`, which introduced
`walk_listing`), `collect_php_files` listed the analysed directories eagerly before
`vendor.get()`, so listing overlapped discovery (`7be53b3c` made it so).

**Now.** `PendingVendor::spawn` (`crates/composer/src/lib.rs:49`) starts the discovery thread
(thread 11, `discover_vendor_context`, 393 M) when the session is built. `walk_listing`
(`session.rs:1083-1091`) joins it with `vendor.get()` before `walk.finish` starts any walk,
because `Walk::add` (`walk.rs:513`) only queues a job. The comment at `session.rs:1081` ("was
being found while the analysed files were listed") is stale.

**The constraint.** `Starts::of(&jobs)` (`walk.rs:235`) needs every root before any file is
handed to the sink, since a file is streamed only when one walk alone lists it. No path prefix
bounds a late root: discovery returns directories outside `vendor/` too (the root package's
own, `$baseDir . '/app'`, `vendor.rs:20`, and path-repository packages).

**Change.** The rule uses the role order instead (`walk.rs:45`: `Context < Stub < Analysed`).

- Today a directory's files are handed over when exactly one walk reaches it (`Reach::Sole`,
  counting every walk). The new rule: **a walk hands over a directory's files when it is the
  only walk of the strongest role that reaches the directory.** A weaker walk that also
  reaches it then neither changes a file's role (the strongest wins) nor hands it over (it is
  not the strongest), so no file is handed twice, and no "handed" set shared by all tasks is
  needed.
- The per-file check follows the same rule. In a sole-reach directory `walk_dir` hands a PHP
  file over only if no file root names it (`!self.starts.files.contains_key(&child)`,
  `walk.rs:763-764`); that becomes "no file root of the same or a stronger role"
  (`Starts.files` keeps each file root's roles), and a file root's own job hands its file over
  by the same rule. Otherwise an analysed walk's decision would depend on whether the vendor
  file roots (1,744 on Piwigo, all `Context`) had been added yet.
- The analysed and stub roots come from the configuration, so they are known before discovery
  ends. `walk_listing` adds the analysed ones first (`session.rs:1073-1080`) and the stub ones
  last (`:1098-1100`, after the vendor roots at 1083 to 1091 and the scan roots, which are
  `Context`, at 1092 to 1097); the row adds the stub roots before `vendor.get()` too. Then the
  analysed and stub walks' `Starts` and `covering` (`finish`, `walk.rs:564`: roots above
  `Context` with no excluded names) are complete without discovery. A directory that no
  analysed or stub walk reaches, and the context walks (the vendor and scan roots), need every
  context root and wait for `vendor.get()`.
- `prunes` (a context walk leaves out what a covering root holds) uses only `covering`, so it
  is unaffected. Link targets (`walk_target`) hand nothing over, as today.
- `finish` runs its jobs in `pool.in_place_scope`, where today it uses
  `pool.install(|| rayon::scope(...))` (`walk.rs:572-573`, which runs the closure on a worker).
  The closure runs on the calling thread: it spawns the analysed and stub jobs, blocks in
  `vendor.get()` without taking a worker, then spawns the context jobs, and the scope's end
  waits for all of them.
- `merge` keeps today's job order (analysed, vendor, scan, stub): each job carries its place in
  that order, and its result goes to that slot, whatever order the jobs start in.
- Fix the stale comment at `session.rs:1081`.

**Gain.** Up to about 0.13 s warm (8% of 1.70 s): the analysed files' fresh parses (579 of the
parse's 1,116 samples, 1.16 s of CPU) can fill the wait. Only the analysed and stub files
(Piwigo checks 2,792 of the 14,251 files it lists) can start before discovery ends, and in
warm mode they are the only files that are parsed rather than read from the cache. At most
0.07 s off.

**Test.** The `walk_identity.rs` cases with a context root added after the analysed ones
produce the same files, roles, hand-overs and listed order as with all roots added at once; a
context root nested in an analysed root hands nothing twice; a stub root that now starts
before the vendor roots leaves the merged order unchanged; and a vendor file root inside an
analysed directory is handed over once, by the analysed walk. The Piwigo oracle runs right
after this row ("Verification").

**Measure.** The warm vendor wait and the parse that now overlaps it in a perf recording, and
hyperfine medians, warm.

### 12. R5: per-file tables keyed by `FileId`

*LOAD-8.5. An Ir row. Needs nothing.*

**Cost.** About 27 M: two sites hash every file's path, 14,251 times each and 13.65 M each. One
of them is item 5's one defect (its `Path::hash`, +13.65 M of its +42.4 M). Time: a few of
`file_tables::bucket`'s 81 samples, which are mostly the merge's routing itself (`tag`, page
faults).

**Now.** `file_tables::bucket` (`shard_of_key(path)`, item 5's merge) routes
`UseSiteIndex.by_file: FxHashMap<PathBuf, ...>` (`use_sites.rs:62`) by path, and
`Project::judge_property_types` (`project.rs:2358`) tests a `checked: FxHashSet<&PathBuf>`
against every file.

**Change.** `FileTables` already has `id`: route and key `by_file` by `FileId`, with the
editor's path lookup resolving the id once (`id_of`). `check_files` results carry the
`FileId`, so `checked` is a set of ids. The call at `project.rs:1013` (`id_for(table.path(id))`
in `number_files`) is not on a fresh batch load (`has_numbers()` is false); it takes the
table's stored hash anyway, for the incremental path.

**Test.** No behaviour changes. Its check is the `project::` tests, among them
`project::tests::file_tables` (the sharded batch leaves the tables as the serial inserts do)
and `use_site_index.rs` (a file's use sites, and dropping them when the file is updated).

**Measure.** Off callgrind: `file_tables::bucket` and `judge_property_types`.

### 13. R8: identity is a `FileId` inside the scope that owns the table, and the resolved path where the scope spans tables

*LOAD-8.8. An Ir row. Needs R5.*

**Cost.** About 34 M of hashing, plus a `PathBuf` clone per result; older than items 1 to 3.
By site (inclusive edge Ir, off): `run_finishing_passes`' result set 14.2 M, `checker_for`
7.7 M, `check_project`'s diagnostics map insert 5.6 M plus its rehash 2.7 M,
`apply_inline_ignores` 2.7 M, `finalize_results` 1.7 M. Time: `checker_for`,
`judge_property_types`, `finalize_results` and `apply_inline_ignores` are 6 + 2 + 2 + 1
samples off and 5 + 2 + 1 + 1 warm. The case is not the size: identity belongs to one type,
and it is converted to display text once.

**Change.**

- **Before `finalize_results`, diagnostics are keyed by `FileId`:**
  - the per-file checks (`project.rs:2626`). `check_files` hands `checker_for` the `FileId` it
    already holds, so `checker_for`'s `self.files.id_of(path)` and its `path_hash` (7.7 M, one
    per checked file) go; the editor's `checker_for(&Path)` entry keeps `id_of`;
  - `apply_inline_ignores` (`:784`), which then hashes once per analysed file;
  - the whole-program passes (`:2712-2873`), which already hold a `FileId` and call
    `self.path_of(file).to_path_buf()`;
  - `class_shape`, whose `files.sort_by` on the path (`class_shape.rs:100`) becomes a sort by
    `rank_of(id)`; `banned`, `mock_hygiene`, `architecture` and `composer_hygiene`;
  - the parse failures (`session.rs:1710`), which come from listed files, so they are already
    numbered;
  - `judge_property_types`' `checked` set (R5), and `run_finishing_passes`'
    `FxHashSet<PathBuf>` (`project.rs:1507`, 13,131 hashed inserts).
- **`FileId` already means "a path the table has numbered"** (`file_table.rs`'s doc: a slot
  keeps its path with `value: None`, and `iter` and `iter_with_ids` skip empty slots). So
  there is no `Subject` enum and no side channel: a path that is not a PHP file is numbered
  too. The one such subject found is `composer.json` (`composer_hygiene.rs:450`, `:499`,
  `manifest.path`); the paths of `packages().manifests()` are numbered at load.
- **The rank seed must include them.** `FileTable::set_path_order` (`file_table.rs`) returns
  without seeding unless `ids.len() == self.slots.len()`, and `number()` drops the seed
  (`ranks.take()`). Numbering the manifests after the files, or anywhere without adding them to
  the order, silently brings back the path sorts that LOAD-4.4 removed (about 20 M each). So
  the manifests' ids are merged, by path, into the ids that `ids_in_listed_order` gives
  (`session.rs`, `project.set_path_order(...)`).
- **`finalize_results` converts once:** a file's key becomes its root-relative text from the
  table (`path(id)`, no hashing). After it the report maps stay path-keyed: the key is the
  root-relative text that `ignore_errors::filter` and `baseline::filter` match, and
  `output.rs`, `baseline.rs`, `ignore_errors.rs` and `output/codeframe.rs` (10 sites) are
  unchanged.
- **The language server** keys its per-project state by `FileId`: `Slot.file_results` and
  `Slot.whole_results` (`lsp.rs:300-306`), converted in `finalized`. Server-wide state stays
  keyed by the resolved path: `open_uris`, `last_diagnostics`, `file_versions`,
  `inlay_hint_cache`, `aliases` and the debounce maps span projects, and a path can be held by
  several (`publisher` and `home_of` pick one of the homes that hold it), each with its own
  `FileId`.
- **Read in the code:** every producer of path-keyed diagnostics, by what its key is (files in
  the table: all; listed but unparsed files: `session.rs`'s parse failures; not in the table:
  only the manifest). `class_shape`, `banned`, `mock_hygiene` and `architecture` all iterate
  `self.files`, so their keys are files in the table.

**Test.** The `path_places.rs` tests assert that the rank seed is kept with the manifests
numbered. The Piwigo oracle runs right after this row, since its key change touches every
diagnostic producer ("Verification").

**Measure.** Off callgrind: the sites above.

### 14. R9: the bytes hashed once in the editor

*LOAD-8.9. The editor's. Needs nothing.*

**Cost.** The cache key's hash is 24.1 M in the warm Piwigo run, in `read_indexed` (XXH3-128:
`Hasher::write` 17.9 M, plus 2.4 M of `finish_128`). `content_hash` adds the same again only
when `retain_source` is on, that is in the editor: once at workspace open and once per edit of
one file. The batch run never takes it.

**Now.** `content_hash` (`source.rs:59`) and `file_cache::key` (`file_cache.rs:237`) both run
XXH3-128 over the same bytes when `retain_source` and the cache are on: `key` runs
`hasher.write(bytes)` over the whole file.

**Change.** The key hashes the content hash with the path and flags. The cache is per build
(`file_cache::trim_builds` keeps the latest builds), so a changed key starts a cold directory,
exactly as any rebuild does; nothing else is orphaned.

**Test.** A file's key changes when one of its bytes does, and stays when none does.

**Measure.** The row has no effect on the batch run, so it is measured in the editor's
workload, `phpstan-rs incremental-bench <dir>` (the project's profiling table; for example
`vendor/symfony/src/Symfony/Component/Console`), at workspace open and per edit, after checking
that the bench runs with `retain_source` and the file cache on.

### 15. R7: the remaining component walks on the load path

*LOAD-8.7. An Ir row, last. Needs R2 to R6 and R8.*

**Cost.** The 481 M of path handling leaves about 266 M outside `Path::hash` that no row
accounts for by caller (`Components::next`, `parse_next_component_back`, `next_back`,
`as_path`, `strip_prefix`, `parent`, `starts_with`, `file_name`). Time: `Components::next` is
4 leaf samples in the off run.

**By caller.**

- Table 4 (the inclusive edge cost of the component-family callees, internal std callers left
  out): the `drop_files_under_dirs` retain closure 50.2 M and `Resolver::path` 34.1 M (both
  R6's); `walk::path_hash` itself 37.1 M (the fold R2 keeps, once per path);
  `Starts::reaching` 30.1 M and `prunes` 5.0 M (R3's); `Session::load` 24.0 M, `Walk::merge`
  10.0 M, `finalize_results` 4.7 M, `PestConfig::resolve_this_type` 4.3 M, `PestFacts::of`
  3.9 M, `lexically_normalized` 3.4 M (vendor) + 2.5 M (manifest),
  `PackageIndex::owner_of_dir` 2.8 M, `Walk::run` 1.6 M, `add_file` 1.3 M, `Tree::node`
  1.0 M, `Resolver::listed` 1.0 M: about 217 M.
- R6 and R3 own 119 M of it, so R7 owns about 98 M, of which `walk::path_hash`'s 37.1 M stays
  (the one fold per path that R2 keeps): about 61 M for R7 to remove.
- Widening the callee family to `extension`, `file_name`, `components`, `join` and
  `compare_components` (a second pass over the same files) attributes about 36 M of the
  remaining ~50 M of the 266 M: `read_indexed` 5.4 M (the file name and extension checks);
  `entry_canonical` 6.0 M; `FileTable::find` 3.8 M (its `Components::eq` 3.2 M and
  `components` 0.6 M, the path equality of a lookup; by caller, `checker_for`'s `id_of` 1.3 M
  and `map_on_pool_largest_first`'s `sort_by_key` 1.3 M, both R8's, whose check list carries
  `FileId`s, and `number` 1.2 M, R2's); the `(&PathBuf, &FileState)` sort 2.5 M
  (`Project::check_class_shapes`, `class_shape.rs:100`,
  `files.sort_by(|a, b| a.0.cmp(b.0))`: a path comparison that LOAD-4.4's rank seed does not
  reach; R8 sorts it by `rank_of(id)`, as `project.rs:2371` and `:2856` do); `listable_dir`
  2.0 M (R6's); `Walk::add_file` 2.1 M (R14's); `Tree::node` 2.4 M (R2's); and larger
  readings for the retain closure (57.6 M, 7.4 M over Table 4) and `Starts::reaching`
  (34.9 M, 4.8 M over) (R6's and R3's). About 13 M stays unattributed.

**Known sites.**

- `read_indexed`'s 5.4 M is `Project::full_path` (`project.rs:965`): `read_into` joins the root
  to every path it reads, though a listed path is already absolute and resolved (`Path::join`
  of an absolute path returns that path after a components check). R7 reads a listed file by
  its listed path, and keeps the join for the editor's relative paths.
- Per file on the check path, `Project::checker_for` costs 7.7 M (`id_of`'s `path_hash`),
  7.4 M (`absolute_path`'s `lexically_normalized`: `absolute_path` joins the root and folds the
  components) and 6.7 M (`resolve_this_type`: `starts_with` against every pest binding per
  file). R8 removes the first and R7 the others: the absolute path is a field of the table's
  slot, computed when the file is numbered, and the bindings that match a file are found once,
  by the file's carried components, not by a `starts_with` per binding per check.

**Change.** Enumerate every caller from a fresh profile, taken after the rows before it (R2
removes some), and replace each with data the walk already holds: a listed path's directory,
name and root-relative form are known when the entry is listed, so they are carried fields
(`cf5a8ae2` did this for the report).

**Done when** the fresh profile accounts for the 266 M by caller (Table 4's 217 M, the widened
pass's 36 M and the last 13 M), and every caller on the load path is replaced or shown to
belong to another row; `path_hash`'s 37.1 M stays.

**Test.** No behaviour changes; the `project::` tests and the Piwigo oracle at the end are its
check.

**Measure.** The fresh off profile, by caller.

## Not in this plan (checked)

- **`LineIndex::new`'s 115 M of `Utf8Chunks::next`** (`crates/syntax/src/span.rs:126`). It is a
  different cost: it runs only for non-ASCII files (`!src.is_ascii()`), but then over the whole
  file, so one multibyte character is enough (Piwigo's 1.8 MB `autoload_static.php` has one
  byte >= 0x80 and costs 12.7 M). It is the same 115 M at `ee56851c`, so it is not from items 1
  to 3, and the ledger's SYN-3.1 and 3.2 (item 11, READY) redesign it: line starts for every
  file, column tables for analysed files only. R1 gives those rows validated text; they own
  the change.
- **Validating the whole file once.** The lexer takes bytes and an optional validated `&str`
  (`crates/syntax/src/lexer.rs:19-24`), so that names and comments are slices of it; with no
  `unsafe`, any other way is a check per token.
- **Item 5's sharded merge.** It exists for wall time. Step 0 measured it at `aa896697` (0.06 s
  off, 1.01 to 1.07 s, at about 3 cores, 92 samples; inside 0.70 to 0.75 s warm), but not
  against the base's serial merge, so whether it pays for its +42.4 M off (−35.6 M warm) is not settled
  here. Its one defect, the `Path::hash` per file (+13.7 M), is R5's.
- **The eight fixes** (−205.5 M) are a gain.
- **The check's unused capacity** (0.172 s warm, 0.089 s off). Inside its stretch the check
  runs at 5.3 to 7.7 cores warm (1.20 to 1.55 s) and at 7.3 to 7.6 off (1.60 to 1.80 s), under
  load 2 to 2.5.
  - What it leaves is its ramp and its tail. The ramp starts as the finishing passes end,
    which is REG-7's (decision 1).
  - In the tail the largest files finish last, though `check_files` already starts the
    costliest first (`map_on_pool_largest_first`, `project.rs:2299`, by source length).
  - So no row here owns it.
- **The CPU time's growth with the thread count** ("Where the CPU growth goes"). It is the VM's
  shared cores and memory, the zeroing (R11) and purging (R15); rayon's spinning is 0.04 s of
  it.
- **The warm run's checksums.** `crc32fast::hash` is 33.5 M warm (26.1 M in `read_indexed`,
  where each cached file's entry is checksummed on load, and 7.8 M in the stub cache). The
  ledger owns it: CACHE-10 gives each record its own CRC, checked when the record is read, and
  CACHE-13's `stat` index replaces these reads (MEAS-1 counts `crc32` 30.4 M among them). The
  checksum itself stays: a cache read must be checked.
- **The finishing passes' unused cores and the stub-base wait** are REG-7's and the STUB rows'
  (decisions 1 and 5).

## Verification

**Per row.** `cargo fmt` first, then `cargo check`, `cargo clippy -D warnings`, then the
targeted tests in the debug profile (`cargo test -p phpstan-rs --lib project::`,
`-p phpstan-rs-composer`); no full suite per row.

**Once, at the end of the batch.** `cargo test -p phpstan-rs --lib` and `--test fixtures`, and
`check-project vendor/monolog` stdout identical between a worktree at `aa896697` (with
`vendor/phpstan-src` copied in, its own target dir, removed afterwards) and HEAD, both sorted
with `LC_ALL=C sort`, with the file cache off, then cold and warm.

**The Piwigo identity oracle.** `check-project --config phpstan-rs.toml --format json` in
`/home/torres/piwigo17-frozen` writes 171,439 bytes, sha256
`8ff144c3fee775166369b993a58814e0f247845fbf850f94d0c41b57924488b1` (2,792 files checked, 581
diagnostics), the same at `d870ddcb` and `aa896697`, in off and warm modes. It must reproduce
byte for byte: no row here changes a diagnostic (R8 changes keys only). It runs once at the end
of the batch (not per row; one heavy job at a time), and also once right after R10, whose rule
decides which walk hands each file to the parse, and once right after R8, whose key change
touches every diagnostic producer, so that a break in either is found before the next rows
build on it.

**Measurement.** Builds and runs need no approval (since 2026-10-17). Heavy jobs run one at a
time, under `nice -n 19`, with the load average logged; another session's heavy job beside
ours is noted, and a run it distorts is taken again.

- **Ir:** off-mode callgrind on Piwigo with the commands of Appendix B, against the previous
  row's binary, built again and measured in the same way (`aa896697`'s code measured
  21,329,725,495 so, against Step 0's 21,331,572,685), and the warm run where a row's Measure
  names it (R1, R6, R12, R13); R9 is measured in the editor's workload. Judge by the inclusive
  Ir of the functions changed (callers with `scripts/callers.py`, differences with
  `scripts/cgfn.py`; Appendix C), with the total beside it.
- **Wall time:** a perf recording in the mode the row targets (off for R11 and R15; warm for
  R10 and R12; both for R14), read for the stretch the row shortens against Step 0's tables
  and against the previous row's recording (Appendix D's commands); and `scripts/abab.py` on
  `gate` binaries, the previous row's against the row's: 20 pairs whose order alternates
  (A B, B A, ...), every output checked against the oracle, judged by the paired difference
  with a Wilcoxon signed-rank test, with each run's peak RSS and CPU time beside it and the
  load average noted. A row's "hyperfine medians" are these. One run decides nothing: the
  6.35 s recording was compaction ("Memory compaction").
  - **Not hyperfine.** R1's first comparison ran hyperfine's two rounds in alternating order
    (10 runs each) under load 8.5 to 14.6 from another session, and σ was 0.85 to 1.28 s in its
    first round. In the rerun (load 6.6 to 8.3) the second command of each round ran slower
    than the first, whichever binary it was: medians 2.052 then 2.247 s, and 2.024 then
    2.198 s. The pairs give each order the same share of the machine's drift.
- **Claims:** an Ir row (R1 to R9, R13) is called an Ir saving, with its wall time shown not to
  regress beyond the noise; a wall row (R10 to R12, R14, R15) is called a saving on the stretch
  it shortens and on the medians, never on Ir alone.

## Bookkeeping

- `WORK_STACK.md`: level 9 tracks this plan (added 2026-10-17, above level 8) and names the
  ledger rows. Delete the level when the plan is finished (the project rule: the level's own
  documents keep the record).
- Done on 2026-10-17, after the rulings:
  - the ledger's rows: LOAD-8.n is Rn (LOAD-8.10 says that it undoes LOAD-1.2's wait); R11 is
    LOAD-3.3's load part, and R12 is CACHE-12, whose decode context is now the file's number,
    so it no longer waits on CACHE-11; LOAD-8.11 and 8.12 point to them;
  - Step 0 as the ledger's MEAS-4, with a paragraph in §1 beside the Ir ranges;
  - the finishing passes' unused cores in REG-7's notes (decision 1), and the stub-base wait
    in STUB's notes, for STUB-10.1 (decision 5);
  - item 6's R5 (the ledger's Appendix B.1) and REG-7.2 cited R12's carried fact; since R12
    carries none (its Change), they say instead that the scan's decodes stay until REG-7;
  - the LOAD-2.2 name floor in the ledger and in `WORK_STACK.md`'s item 2 (decision 7).
- Done with R1 and R11: LOAD-3.4's note and LOAD-6's record; LOAD-3.3's record, which says
  that `update_file` waits on LOAD-3.1; their rows in §1's table; CACHE-12's row, for R12's
  corrected change.
- Done with R12: CACHE-12's task, target and record (its decode context is the file's
  number, so E8's was not needed); its row in §1's table; §1's open off-drill rows and §2.1.
- Done with R15: LOAD-8.15's row, with the `unsafe` function and the smaller saving; its row
  in §1's table, whose Ir comes from R2's baseline run.
- Done with R2: LOAD-8.2's row, with the parent's hash the path carries; its row in §1's table;
  LOAD-8.3, 8.4 and 8.6 are READY.
- Left, with the rows: the stale comment at `session.rs:1081` (R10); §1's range rows, as each
  row lands.
- Another session shares the checkout: `git status` and `git log` before starting; commit with
  a pathspec; remove scratch worktrees once they are recorded.

## Reuse

`Listed.hash` and `child_hash` (the walk's incremental hash); `FileTable`'s
`ids: HashTable<FileId>` and `number(path, hash)` (`file_table.rs`); `FileIndex::of`
(`walk.rs:105`); `Listing::entry` and `DirListings` (`dir_listings.rs`); the tests
`load_text.rs` and `walk_identity.rs`; the scripts in `scripts/` (`callers.py`, `cgfn.py`,
`perfdump.py`, `cpugrowth.py`, `fit.py`, `hfpool.py`, `thp-probe.py`, `scaling.sh`,
`mimalloc-runs.sh` and the `probe-vendor` crate) and the commands of the Appendix.

## Measurements: Step 0 (2026-10-17)

### Setup

- **The binary:** `head-cg`, the `dhat` profile with frame pointers, built from `aa896697`'s
  code (sha256 `118e5dcc…`; Appendix A). Wall times and perf samples come from that same
  binary; the scaling set and the allocator's hyperfine runs use a `gate` build of the same
  code.
- **The command:** Piwigo, `check-project --config phpstan-rs.toml --format json`. Every run
  whose output was kept wrote the oracle's output, sha256 `8ff144c3…`: the two off
  recordings, the warm recording, the strace run, and the plain off run with the
  `/proc/vmstat` counters. The six plain timing runs below wrote to `/dev/null`, and the gate
  binary's scaling runs were not hashed (Appendix D).
- **The machine is a virtual machine:** 8 vCPUs, one thread each, on an Intel i7-8700 host,
  which has 6 cores and 12 hardware threads, so when all eight vCPUs are busy at least two
  cores run two of them each. It exposes no hardware performance counter (`perf stat` reports
  `cycles` and `instructions` as not supported, and `perf record` samples with `task-clock`)
  and no cpufreq interface, so clock speed and cache or memory contention cannot be told apart
  here. A "core" below is a vCPU.
- **The load:** the 1-minute load was 2.26 before the off recording read here and 2.20 before
  the warm one (1.5 to 2.5 at the other steps), with other sessions' processes present.
- **The recordings:** `perf record -F 499 -g` (frame pointers), so one sample is 2 ms of one
  thread's CPU. The off recording has 5,061 samples, the last at 2.018 s (main thread tid
  75945, vendor thread 75969, the eight pool threads 75951 to 75961); the warm one 3,391
  samples, the last at 1.703 s (main 79694, vendor 79704). Each was dumped once with
  `perf script -i <file> --no-inline -F tid,time,ip,sym > <dump>` (about 1 s) and read with
  `scripts/perfdump.py` (Appendix D lists the command behind each table). Times are from each
  recording's first sample.
- **What `-g` costs** at 499 Hz is not resolved by one run each: off runs under
  `perf record -F 499 -g` and `-F 499` took 2,626 and 2,577 ms, and one under `-F 199 -g` took
  3,200 ms.

### Wall time without perf

Off 2,310, 2,296 and 2,878 ms; warm 1,769, 1,602 and 1,568 ms. The warm runs rewrote no cache
file. Load 2.08 to 2.17 before the off runs (not logged before the warm ones).

### Memory compaction

- **One off recording took 6.35 s.** Its vendor thread ran 1.6 s, with its stack in
  `io_schedule`, `migrate_pages` and `try_to_compact_pages` (direct compaction), not in
  discovery, and its pool idled from 0.75 s.
- **This machine compacts memory during the load.** In one ordinary run `/proc/vmstat` shows
  `compact_stall` rising by 1,076 and `pgmigrate_success` by 323,269. The counters are
  system-wide, so other processes count in them too (`compact_fail` rose by 983 and
  `compact_success` by 93 in the same run); the vendor thread's own stack is what ties the
  slow run to compaction. The ordinary off run compacts too: `isolate_migratepages_block` and
  `migrate_folio_*` frames sit in the shard merge's samples.
- **Discovery alone is fast.** An isolated `discover_vendor_context` (the probe
  `scripts/probe-vendor`, 7 runs; Appendix D) takes 103.5 to 133.5 ms (median 109.5).
- **The run itself asks for the high-order pages.** mimalloc gives its 1 GiB arenas
  `MADV_HUGEPAGE`, an off run takes about 550 transparent huge pages, and with THP `defrag` at
  `madvise` a huge-page fault in a madvised region compacts memory directly when no free 2 MiB
  page is at hand ("Kernel time").
- So every row's wall claim is a median of repeated runs, with the compaction counters logged,
  not one recording.

### The off timeline (2.02 s under perf)

The order comes from the main thread's own samples (`perfdump.py main`), and the cores from the
role mix per 0.1 s window (`perfdump.py phases`).

| time (s) | cores | what runs |
|---|---:|---|
| 0.000 to 0.042 | | main thread: configuration, manifests |
| 0.00 to 0.21 | 5.5 to 5.7 | stub build on the pool (`build_generated_stub_data`, 4.7 to 4.8 cores); vendor discovery on its own thread from 0.041 to 0.208 (73 samples); the main thread is blocked in `vendor.get()` from 0.042 to 0.210 |
| 0.210 to 0.222 | | main thread adds the vendor roots (`Walk::add`, `add_file`: the second resolution, R14; 7 samples) |
| 0.22 to 0.90 | 6.0 to 6.9 | streaming parse (`read_indexed` under `Walk::run`); its tail, 0.8 to 0.9, runs at 4.5 cores |
| 0.898 to 0.917 | | main thread: the hand-over (R4), then `bind_file` |
| 0.917 to 1.006 | 2.2 | main thread blocked in `Stubs::base` (asked by `expect_files`): the stub-base thread (0.921 to 1.003, 20 samples: copies and a sort of `(Name, Arc)` pairs) runs `generated_stubs`, `merge_stubs` and `freeze`, and the base's finishing passes run on the pool (0.93 to 0.97) |
| 1.006 to 1.032 | | main thread: `split_decl_tables`, `join_decl_tables` |
| 1.01 to 1.07 | about 3 | `merge_by_shard` on the pool (92 samples, 81 of them under `file_tables::bucket`) |
| 1.068 to 1.080 | | main thread: `join_project_tables` |
| 1.080 to 1.162 | 1.3 | main thread: `insert_numbered_file`, 41 samples (82 ms), 36 of them freeing the context trees (72 ms, R11), 3 in `mentions_phpstan` |
| 1.16 to 1.52 | 1.9 to 2.9 | the project's finishing passes: the thread in `run_finishing_passes` is busy throughout (5 samples per 10 ms; `is_folded_eq` and `fold_hash` lead its leaves), and the pool runs its stolen jobs (mostly `flatten_phpdoc_inheritance_one_pass`) |
| 1.52 to 2.02 | 5.9 to 7.6 | check (`check_files`); `finalize_results` at 2.01 |

### The warm timeline (1.70 s under perf; 0.05 s windows)

| time (s) | cores | what runs |
|---|---:|---|
| 0.000 to 0.027 | 0.9 | main thread: configuration |
| 0.027 to 0.147 | 0.9 to 1.0 | main thread blocked in `vendor.get()` for 120 ms; the vendor thread (0.029 to 0.144, 56 samples) is the only one running, and the pool's first samples are at 0.164 s (R10, R6, R13) |
| 0.147 to 0.161 | | main thread adds the vendor roots (R14; 8 samples: 5 under `Walk::add` and `add_file`, 3 right under `walk_listing`, which is inlined: a `Path::join`, a `Components::eq` and a copy; `is_promoted` returns at once, since Piwigo has no path repositories) |
| 0.16 to 0.58 | 4.2 to 6.8 | streaming parse: cache hits for the context files, fresh parses for the analysed ones; 2.6 cores in its tail (0.55 to 0.60) |
| 0.580 to 0.592 | | main thread: the hand-over (R4) |
| 0.592 to 0.702 | 1.0 | main thread alone in `bind_file`, 55 samples (110 ms, R12); the pool is idle |
| 0.70 to 0.765 | 2.1 to 2.9 | the main thread waits about 9 ms for the stub base, read from the stub cache (no main-thread sample from 0.702 to 0.711; 3 samples on the reading thread); `merge_by_shard`, then on the main thread `join_decl_tables`, `join_project_tables` and `insert_numbered_file` (8 ms) |
| 0.75 to 1.19 | 1.0 to 3.5 | the project's finishing passes; rayon's idle frames hold 0.2 to 1.7 cores of the pool meanwhile |
| 1.19 to 1.70 | 5.0 to 7.7 | check, with a ramp (1.15 to 1.20) and a tail (1.60 to 1.70) |

### Where the 8 cores go unused

`perfdump.py unused` gives, per stretch, (8 x span - CPU seconds) / 8, so the stretches sum to
the run's wall time less its CPU time / 8. Load was 2 to 2.5, so other processes took part of
this capacity; the figures bound what a row can gain.

| off stretch | cores | unused | warm stretch | cores | unused |
|---|---:|---:|---|---:|---:|
| 0 to 0.21: stub build, discovery | 5.46 | 0.067 s | 0 to 0.15: the vendor wait | 0.95 | 0.132 s |
| 0.21 to 0.90: parse | 6.15 | 0.160 s | 0.15 to 0.55: parse | 5.71 | 0.115 s |
| 0.90 to 1.50: stub base, merge, numbering, finishing passes | 2.20 | 0.435 s | 0.55 to 0.70: `bind_file` | 1.51 | 0.122 s |
| 1.50 to 2.02: check | 6.63 | 0.089 s | 0.70 to 0.75: merge | 2.89 | 0.032 s |
| | | | 0.75 to 1.15: finishing passes | 2.37 | 0.281 s |
| | | | 1.15 to 1.70: check | 5.52 | 0.172 s |
| whole run (2.018 s) | 5.03 | 0.750 s | whole run (1.703 s) | 3.99 | 0.853 s |

**The finishing passes** run from 1.16 to 1.52 s off and from 0.75 to 1.19 s warm, at 2 to 3
cores: the thread in `run_finishing_passes` is busy throughout, and the pool takes only part
of its work. Their leaves are mostly `flatten_phpdoc_inheritance_one_pass`'s closures and
`FoldKey` work (`is_folded_eq`, `fold_hash`), and 29% of the samples in the off run's 0.8 to
1.5 s stretch are kernel time (mostly task switches; TLB-shootdown IPIs only 1%). They leave
about 0.26 s (off) and 0.28 s (warm) of the 8 cores unused, more than any row here reaches
(decision 1).

**The stub-base wait** is a cold-stub-cache cost. In the off run the main thread is blocked
89 ms in `Stubs::base` (0.917 to 1.006 s) while the base is merged, frozen and finished; the
warm run reads the base from the stub cache, and the main thread waits about 9 ms for it
(0.702 to 0.711 s). So most of the wait is what a run after a rebuild, or with the stub cache
off, pays (decision 5).

### The main thread

CPU samples at 499 Hz, inclusive.

| part | off | warm | Ir |
|---|---:|---:|---|
| `Session::load` (the main thread's share of the load) | 160 ms | 172 ms | 424.1 M off, 537.2 M warm (thread 01: 538.7 M, 651.7 M) |
| `insert_numbered_file` | 82 ms (freeing the context trees 72 ms) | 8 ms | 281.6 M (R11: 265 M) |
| `ParsedFile::bind_file` | 10 ms | 110 ms | 401 M warm (R12: 393 M) |
| from the parse's end to `bind_file` (the hand-over, R4) | 4 samples (8 ms), `Path::hash` 1 of them | 4 samples (8 ms), `Path::hash` 2 | 27.3 M |
| adding the vendor roots (R14) | 14 ms (7 samples) | 16 ms (8 samples) | |
| blocked, no CPU | `vendor.get()` 168 ms (the pool busy with the stub build); `Stubs::base` 89 ms | `vendor.get()` 120 ms (the pool idle); `Stubs::base` about 9 ms | |

`into_parsed` is inlined into `Session::load` (callgrind's `Path::hash` edge for it comes from
`load` itself), so the hand-over's samples are `load`'s own code and its direct `Path::hash`.
The main thread is busy for 0.22 s of the 2.02 s off run and 0.21 s of the 1.70 s warm run (110
and 106 samples). The rest is waiting, so a main-thread row gains wall time only where the main
thread gates the next phase: R11 (off), R12 (warm), R4 and R14 (both), and the two waits.

### Ir is not time

Perf leaf samples against callgrind's self Ir for the same functions (off run, same binary; one
sample is 2 ms of CPU):

| function | self Ir | leaf samples | CPU | Ir per CPU second |
|---|---:|---:|---:|---:|
| `_mi_theap_malloc_zero` | 314.9 M | 163 | 326 ms | 1.0 G |
| `Lexer::name_and_bits` | 368.3 M | 89 | 178 ms | 2.1 G |
| `mi_free` | 341.3 M | 77 | 154 ms | 2.2 G |
| `memcpy` (glibc's `__memmove_avx_unaligned_erms`) | 634.4 M | 86 | 172 ms | 3.7 G |
| `Lexer::next_token` | 835.1 M | 90 | 180 ms | 4.6 G |
| `Parser::fill` | 567.0 M | 28 | 56 ms | 10 G |
| `Utf8Chunks::next` (all callers) | 532.5 M | 23 | 46 ms | 12 G |
| `Components::next` | 96.2 M | 4 | 8 ms | 12 G |
| `Path::hash` | 194.3 M | 3 | 6 ms | over 10 G (3 samples) |

Allocation, copies and the lexer's name work run at 1 to 5 G Ir per CPU second; the tight byte
loops that R1 to R8 target (UTF-8 validation, path hashing, component walks) at 10 G or more.
So the time each row's code takes in these runs (inclusive samples):

| row | code | off | warm |
|---|---|---:|---:|
| R1 | the lossy decode (leaf `Utf8Chunks` right under `read_indexed`) | 21 (42 ms of CPU over the pool, about 7 ms of wall at the parse's 6 cores) | 11 (22 ms) |
| R2, R3, R7 | `Starts::reaching`, `Walk::merge`, `Tree::node`, `walk::path_hash`, `prunes` (overlapping) | 4 + 5 + 2 + 5 + 1, at most 17 | 6 + 6 + 3 + 3 + 0, at most 18 |
| R5 | `file_tables::bucket`'s path hash | a few of its 81 samples, which are the merge's routing itself (`tag`, page faults) | |
| R8 | `checker_for`, `judge_property_types`, `finalize_results`, `apply_inline_ignores` | 6 + 2 + 2 + 1 | 5 + 2 + 1 + 1 |
| R2 to R8 | `Path::hash`, anywhere | 3 | 12 |
| R6 | the retain closure (`drop_files_under_dirs`): mostly `statx` and dentry lookups, the per-file `symlink_metadata`; `Resolver::path`, of which `listed` (the directory reads, which stay) | 22; 35, `listed` 28 | 20; 22, `listed` 17 |
| R13 | `parse_generated_file` | 15 (30 ms) | 8 (16 ms) |
| R11 | freeing the context trees in `insert_numbered_file` | 36 (72 ms, main thread) | |
| R12 | `bind_file` | 5 | 55 (110 ms, main thread) |

So R1 to R9 and R13 are Ir rows: R1 to R8 serve the 20 G plan's off-mode Ir total, R13 the
warm run's Ir and R9 the editor's, and each saves a few ms of wall time at most (R9 none in a
batch run). The wall rows are R10, R11, R12, R14 and R15.

### Kernel time

- **The leaves.** 23% of the off run's samples and 22% of the warm run's have a kernel leaf.
  The largest is `__clear_pages_unrolled`, the kernel zeroing a page on its first touch: 460
  samples off (9% of all, 0.92 s of CPU) and 244 warm (7%), the largest single leaf in both
  runs. Then `finish_task_switch` (273 off, 173 warm) and `handle_softirqs`. Strace (off)
  counts 819 `madvise` calls and 13,756 `sched_yield` (rayon's idle loop). Kernel time grows
  with the thread count (`sys` 0.43 s at one thread, 1.82 s at eight, off; "Scaling").
- **The allocator** is mimalloc v3 (`libmimalloc-sys` 0.1.49, whose `build.rs` picks
  `c_src/mimalloc/v3` unless the `v2` feature is on; the workspace sets
  `default-features = false` and no feature; `MI_NO_THP` is unset; our code sets no option)
  with v3's defaults:
  - `purge_delay` 1000 ms and `purge_decommits` on (`src/options.c:127`, `:140`), so a purge is
    `MADV_DONTNEED` (`src/prim/unix/prim.c:498-501`), and a purged page is zeroed again when it
    is reused;
  - `allow_thp` on (`options.c:176-177`).
- **The process gets transparent huge pages.** An OS allocation of at least 8 large pages
  (16 MiB) is aligned to 2 MiB (`src/os.c:303-305`), so the 1 GiB arenas pass
  `_mi_os_canuse_large_page` (`os.c:80`) and get `MADV_HUGEPAGE` (`prim.c:412-418`). The arenas'
  own slice alignment, `MI_ARENA_SLICE_ALIGN`, is 64 KiB; the 2 MiB alignment comes from the OS
  allocation. The huge-page probe below measured them.
- **This machine** has THP `enabled` and `defrag` at `madvise`, so a huge-page fault in a
  madvised region compacts memory directly when no free 2 MiB page is at hand, and every mTHP
  size is `never` except 2 MiB (`inherit`).

### The allocator runs

2026-10-17, 13:36 to 13:41, off mode, every output the oracle's (`scripts/mimalloc-runs.sh`,
`scripts/thp-probe.py` and `scripts/hfpool.py`; Appendix D). mimalloc reads its options from
the environment, so none of these needed a build.

- **mimalloc's statistics at exit** (`MIMALLOC_SHOW_STATS=1`, gate binary, one run each, load
  0.39). With the defaults: 987 purges, 154.8 MiB in all (160 KiB each on average), user
  9.514 s, system 2.863 s. With `MIMALLOC_PURGE_DELAY=-1`: no purge, user 9.306 s, system
  2.507 s. Both commit 1.1 GiB and peak at 1.0 GiB of RSS. So purging lowers no peak: it hands
  back memory freed after the peak, which a batch run holds only until it exits.
- **hyperfine** (gate binary; `-N --warmup 2 --runs 10`; two rounds in alternating order,
  default then no purge, and no purge then default; load 1.00 at the start of the first round
  and 5.93 at the start of the second, raised by the runs themselves). Default 2.258 ± 0.230
  and 2.276 ± 0.249 s; no purge 2.159 ± 0.149 and 2.076 ± 0.182 s. Pooled, 20 runs each:
  medians 2.228 and 2.108 s, means 2.267 (σ 0.234) and 2.117 (σ 0.167). No purge is 0.121 s
  (5.4%) faster on the medians, Mann-Whitney U = 276, p = 0.040 (two-sided, normal
  approximation).
- **perf** (`head-cg`, one recording each, right after the hyperfine set; load 8.4 from the
  set's own runs, with nothing of ours running beside the recordings). Default: 6,335 samples,
  the last at 2.525 s, 21% of them kernel leaves, 104 with a TLB-shootdown or IPI frame
  (`smp_call_function_many_cond` is the leaf of 53). No purge: 5,493 samples, the last at
  2.169 s, none with such a frame. `__clear_pages_unrolled` is 456 and 462 samples, the same:
  the zeroing is not purged pages faulted again.
- **Huge pages.** `thp_fault_alloc` rose by 551 and 544 in the two recordings (a system-wide
  counter), and `compact_stall` by 5 and 4. `thp-probe.py` samples
  `/proc/<pid>/smaps_rollup` every 20 ms over a run (two runs each, alternating, all under the
  probe's sampling). With the defaults, Rss peaks at 1,114 and 1,103 MiB, 1,066 and 1,054 MiB of
  it `AnonHugePages`, with about 550 huge-page faults, and the run takes 2.734 and 2.936 s.
  With `MIMALLOC_ALLOW_THP=0` (mimalloc then sets `PR_SET_THP_DISABLE` for the process,
  `prim.c:222-226`), Rss peaks at 928 and 931 MiB with no huge page, and the run takes 4.125
  and 3.552 s.

What they show:

- **The zeroing is first touch.** It is the first touch of the run's footprint, 1.1 GiB and
  nearly all of it huge pages, so only a smaller footprint lowers it. R11 lowers it: the
  context trees are freed during the parse.
- **The huge pages stay.** Without them the run takes 3.6 to 4.1 s, against 2.7 to 2.9 s.
- **The huge pages are where the compaction comes from.** With `defrag` at `madvise`, a
  huge-page fault in mimalloc's madvised arenas compacts memory directly when no free 2 MiB
  page is at hand; that is what the 6.35 s recording's vendor thread was doing. The machine's
  `defrag` setting decides it, not our code: `defer` takes a huge page only when one is free,
  with no stall.
- **Purging gains a batch run nothing.** It costs about 5% of the wall time and all of the
  TLB-shootdown IPIs: R15.

### System calls

Strace of an off run (its wall time is inflated by ptrace; the counts are exact): 28,581
`read`, 29,161 `statx`, 19,341 `openat`, 19,332 `close`, 14,272 `lseek`, 13,756 `sched_yield`,
7,494 `readlink` (7,493 of them fail with EINVAL), 5,714 `getdents64`, 4,467 `futex`, 2,873
`fstat`, 819 `madvise`. glibc's `realpath`, behind `fs::canonicalize`, calls `readlink` once
per path component, and EINVAL means the component is not a link. The count is not split by
thread. By the code, most of the failing `readlink`s are the main thread's while it adds the
vendor roots (0.210 to 0.222 s off, 0.147 to 0.161 s warm), resolving a second time the paths
that discovery returned mostly canonical already, for 14 to 16 ms before the walk can start, in
both modes. R14 has the detail, and counts the calls per thread.

### Scaling

Gate binary at `aa896697` (built in 6 min 16 s); `scripts/scaling.sh` (Appendix D): hyperfine
`-N --warmup 1 --runs 5`, `PHPSTAN_RS_THREADS=1,2,4,8`. The 1-minute load was 2.62 at the start
of the off set and 3.51 at the start of the warm set, so this is noise, not a quiet machine;
the warm set wrote 0 cache files. Wall in ms; CPU is hyperfine's mean user + system time:

| threads | off mean (σ) | off median | off CPU | warm mean (σ) | warm median | warm CPU |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 6,798 (909) | 6,657 | 6.98 s (6.54 + 0.43) | 4,385 (37) | 4,393 | 4.38 s (3.97 + 0.41) |
| 2 | 3,859 (224) | 3,769 | 7.33 s (6.77 + 0.56) | 2,516 (108) | 2,530 | 4.45 s (3.98 + 0.46) |
| 4 | 2,538 (94) | 2,597 | 8.41 s (7.55 + 0.87) | 1,907 (230) | 1,847 | 5.67 s (4.96 + 0.72) |
| 8 | 2,176 (198) | 2,123 | 11.87 s (10.05 + 1.82) | 1,528 (32) | 1,539 | 7.05 s (5.87 + 1.18) |

Least squares of wall = serial + parallel / threads: on the means, off serial 1,308 ms and
parallel 5,407 ms, warm 1,055 and 3,262; on the medians, off 1,327 and 5,246 (residuals −1.3,
+4.8, +1.6, −6.6%), warm 1,033 and 3,295 (−1.5, +5.9, +0.5, −6.1%). The fit is within 9% at
every point (the worst is off at 8 threads on the means: 1,984 against 2,176). It does not
measure serial work here: the form assumes the CPU time stays the same as threads are added,
and it grows by 70% off and 61% warm from one thread to eight, so the serial term absorbs that
growth. The measure that holds is the timeline's unused capacity: 0.75 s off and 0.85 s warm
at 8 threads ("Where the 8 cores go unused").

### Where the CPU growth goes

Off mode. Two `head-cg` recordings were taken one right after the other at 13:52, at one
thread and at eight: 3,114 and 5,136 samples, the oracle's output both times. Load was 1.9 and
2.1, with another session's test suite on one vCPU. They were read with `scripts/cpugrowth.py`
(Appendix D), which puts each sample in one class by its leaf. An earlier one-thread recording,
at 13:44, is not used: it ran 20.0 s with 5,766 samples (11.5 s of CPU, against 6.2 s in the
pair) at 55 to 77% of one vCPU through the parse and the check; load was 2.64 at its start, and
another session's test binary was running at normal priority when checked right after it. CPU
seconds:

| class of the leaf | 1 thread | 8 threads | growth |
|---|---:|---:|---:|
| our code | 4.858 | 7.018 | +2.160 |
| kernel: page zeroing (`__clear_pages*`) | 0.084 | 0.824 | +0.739 |
| kernel: task switch (`finish_task_switch`, `schedule`) | 0.072 | 0.505 | +0.433 |
| mimalloc | 0.571 | 0.934 | +0.363 |
| kernel: other | 0.240 | 0.403 | +0.162 |
| kernel: TLB shootdown, IPI | 0 | 0.086 | +0.086 |
| rayon idle, spinning (`sched_yield`, `rayon_core` sleep) | 0.012 | 0.052 | +0.040 |
| kernel: softirq, IRQ exit | 0.092 | 0.126 | +0.034 |
| libc `mem*` | 0.265 | 0.297 | +0.032 |
| kernel: futex, `sched_yield`, file system | 0.046 | 0.048 | +0.002 |
| **total** | **6.240** | **10.293** | **+4.052** |

By role: parse 2.222 to 4.052 s, check 2.108 to 3.565, stub build 0.894 to 1.236. The
finishing passes' own thread goes from 0.581 to 0.317 s, while their stolen jobs, which the
role marks as `other`, go from 0.054 to 0.447.

`perf stat -r 3` on the gate binary gives the same picture (13:51, load 1.15). At one thread
and at eight: task-clock 6,580 and 10,418 ms (± 2.8%), wall 6.379 and 1.947 s, context switches
127 and 5,110, CPU migrations 90 and 2,637, page faults 4,606 and 3,878.

So rayon's spinning is not the growth (0.04 s), and neither are locks. The rest:

- **The same code takes 44% more CPU at eight threads:** our code 4.86 to 7.02 s, mimalloc
  0.57 to 0.93 s.
- **The zeroing takes ten times as much:** 0.08 to 0.82 s.
- **Eight busy vCPUs share the host's six cores,** its 12 MiB L3 and its memory bandwidth. This
  VM has no counter that separates clock speed from cache and memory contention.
- **Task switches add 0.43 s,** 0.18 s of it in the idle pool: rayon's workers sleep and wake,
  and at times more threads are runnable than there are vCPUs (the main, vendor and stub
  threads beside eight workers).
- **The TLB shootdowns (0.09 s) are purging's.**

No row owns the growth beyond R11, which lowers the footprint that the zeroing writes, and
R15.

### Warm-cache integrity

The warm runs wrote 0 cache files. They read one cache, a copy of the `regress-item5` runs'
filled cache, and the file times confirm it: all 11,462 files keep the times of the copy
(11:34:40 to 11:34:46), and no file or directory changed after 11:35, so no warm run wrote one:
not the recording at 11:40, not the plain runs, not the scaling set (11:51 to 11:53). A
recording's own check, `find -newer` against its output file, would miss a file written before
the output.

## Callgrind and heap figures (`aa896697` against `d870ddcb`)

**Provenance.** The `regress-item5` harness (Appendix B), 2026-10-17 08:47 to 09:47, checkout
at `aa896697` (clean), base `d870ddcb`; Piwigo (`/home/torres/piwigo17-frozen`); valgrind
3.26.0, callgrind with `Ir` only and `--separate-threads=yes`. Binaries are the `dhat` profile
with `-C force-frame-pointers=yes`. sha256: `head-cg`
`118e5dcc5f85267ee123c534be7a2f1adbd0a318e67065736bffbf8b2546d2be`, `base-cg`
`6a2fe0c5c01903db147cd16daa3e7c248ec3e7d97dae6cb69a02d5cc4caf5bb9`. Build times: `head-cg`
6.5 min, `head-heap` 4.1 min, `base-cg` 6.25 min (load 1.0 to 1.9 at start). A callgrind run
takes 3.6 to 4.3 min (off and warm, HEAD and base). The warm-cache fill runs took 9 s at HEAD
(load 3.7 to 4.2) and 4 s at the base (load 1.9): cold-stub-cache fills taken under load, not
a comparison.

**Table 1: Ir per thread (file `cg.out-NN`).** Off mode is
`PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1`; warm is a `PHPSTAN_RS_CACHE_DIR` of its
own, filled by a run first (the measured run rewrote 0 cache files, with both binaries).

| thread | off HEAD | off base | warm HEAD | warm base |
|---|---:|---:|---:|---:|
| 01 main | 538,727,190 | 642,376,975 | 651,701,904 | 806,601,152 |
| 02 | 1,829,819,564 | 2,299,179,842 | 2,888,446,552 | 3,847,644,989 |
| 03 | 2,693,897,650 | 2,684,364,946 | 2,496,100,624 | 1,089,028,393 |
| 04 | 1,989,962,007 | 2,244,318,643 | 1,040,474,189 | 1,150,772,198 |
| 05 | 4,269,898,686 | 4,052,054,461 | 1,466,328,887 | 1,133,184,219 |
| 06 | 2,797,461,752 | 3,392,701,385 | 495,947,324 | 1,126,380,002 |
| 07 | 1,913,997,312 | 2,440,578,214 | 1,274,041,750 | 823,031,329 |
| 08 | 2,303,840,981 | 1,021,467,253 | 1,728,610,988 | 1,866,385,581 |
| 09 | 2,348,594,153 | 1,866,620,567 | 899,775,564 | 1,134,050,933 |
| 10 | 251,642,861 | 251,750,976 | 405,482,946 | 405,457,124 |
| 11 | 393,308,327 | 393,295,457 | | |
| 12 to 19 | 44 K each (112 K for one) | same | | |
| **total** | **21,331,572,685** | **21,289,129,115** | **13,346,910,728** | **13,382,535,920** |

Head minus base: off +42,443,570 (+0.1994%); warm −35,625,192 (−0.2662%). The thread numbers
do not map to the same work across runs: which pool thread takes which chunk is scheduling.
Roles at HEAD, off mode: 01 `main` (`Session::load` 424.1 M inclusive); 02 to 09 the rayon pool
(the walk's streaming parse, `read_indexed` 867 M to 1,640 M inclusive each; thread 05 also runs
`run_finishing_passes`, 1,003.5 M); 10 the stub builders (`build_generated_stub_data` 131.7 M,
`Stubs::base` 119.6 M); 11 `discover_vendor_context` (393.1 M inclusive). In warm mode the stub
builders do not run, and thread 10 is `discover_vendor_context` (393.5 M inclusive of 405.5 M).

**Table 2: path and decode costs per thread, off mode, HEAD (self Ir unless marked).**

| thread | `Utf8Chunks::next` | `Path::hash` | `Components::next` | `path_hash` (incl) | `LineIndex::new` (incl) | `read_indexed` (incl) |
|---|---:|---:|---:|---:|---:|---:|
| 01 | 1.05 M | 57.4 M | 25.1 M | 15.3 M | | |
| 02 | 65.7 M | 8.0 M | 15.7 M | 24.6 M | 58.6 M | 975.4 M |
| 03 | 88.2 M | 11.0 M | 2.3 M | 2.2 M | 84.7 M | 1,115.1 M |
| 04 | 65.2 M | 18.9 M | 1.8 M | 2.2 M | 46.8 M | 1,069.4 M |
| 05 | 78.9 M | 23.1 M | 4.2 M | 5.5 M | 26.6 M | 1,640.1 M |
| 06 | 48.9 M | 6.5 M | 7.2 M | 5.9 M | 28.5 M | 867.0 M |
| 07 | 51.8 M | 4.9 M | 0.9 M | 0.8 M | 31.0 M | 1,001.1 M |
| 08 | 63.5 M | 4.8 M | 1.5 M | 1.0 M | 32.3 M | 1,235.8 M |
| 09 | 56.6 M | 6.3 M | 1.7 M | 1.2 M | 30.0 M | 1,051.5 M |
| 11 | 12.7 M | 53.4 M | 35.9 M | | 25.0 M | |
| **sum** | **532.5 M** | **194.3 M** | **96.2 M** | | | |

The 532.5 M of `Utf8Chunks::next` is: `read_indexed` (the lossy decode, R1) 394.3 M,
`LineIndex::new` 115.2 M, `PhpString::to_str_lossy` 12.2 M,
`Registry::infer_global_variables_of` 10.6 M, `PhpString`'s `Display` 5.1 M,
`ignore_errors::filter` 1.1 M. In the warm run its callers are `read_indexed` 133.6 M (the
analysed files, which are never cached), `PhpString::to_str_lossy` 11.8 M,
`infer_global_variables_of` 10.2 M, `Display` 5.0 M, `ignore_errors::filter` 1.1 M, and
`LineIndex::new` 46.6 M. Also in the warm run, `read_indexed` is 193 M to 1,845 M inclusive per
thread, and thread 02 holds 105.1 M of `Utf8Chunks::next` (which thread parses the analysed
files is scheduling; the cause of the 105 M is not traced beyond the callers above).

**Table 3: callers of `Path::hash`** (inclusive edge Ir, all threads, off mode; 277,176 calls in
all, about 701 Ir each). `Starts::reaching` 32.4 M; `Session::load` 27.3 M; the
`drop_files_under_dirs` retain closure 23.2 M; `listable_dir` 16.6 M; the `judge_property_types`
`checked` insert 13.65 M; `file_tables::bucket` (`merge_by_shard`) 13.65 M;
`HashMap<PathBuf, ()>::insert` 12.6 M; `DirListings::listing` 9.1 M; `walk_dir` 8.0 M + 3.9 M;
`Walk::prunes` 7.1 M; `HashMap<PathBuf, Vec<Diagnostic>>::insert` 3.5 M (+ its rehash 2.7 M,
`get_mut` 2.2 M); `DirListings`' `OnceLock` table rehash 2.8 M; `Project::check_files` 2.2 M;
`HashMap<&Path, ()>::insert` 2.2 M + rehash 2.1 M; `Starts::of` 2.0 M;
`HashMap<PathBuf, Vec<&[&str]>>` rehash 2.0 M; `Walk::run` 1.8 M; `Walk::add_file` 1.5 M;
`HashMap<PathBuf, bool>` rehash 1.4 M. Callers of `walk::path_hash`: `Tree::node` 20.9 M +
3.5 M, `Project::checker_for` 7.7 M (`id_of`, once per checked file), `Walk::run` 3.2 M,
`walk_root` 0.4 M.

**Table 4: callers of the component functions** (`Components::next`,
`parse_next_component_back`, `next_back`, `as_path`, `_strip_prefix`, `parent`, `_starts_with`,
`file_name`; inclusive edge Ir, all threads, off mode). The edges overlap (std calls std), so
the std-internal rows (`next_back` 43.4 M, `_strip_prefix` 36.5 M, `parent` 25.5 M, `as_path`
20.1 M, `_starts_with` 18.5 M) are not callers to fix. The rows: `drop_files_under_dirs` retain
closure 50.2 M; `walk::path_hash` 37.1 M; `Resolver::path` 34.1 M; `Starts::reaching` 30.1 M;
`Session::load` 24.0 M; `Walk::merge` 10.0 M; `Walk::prunes` 5.0 M; `finalize_results` 4.7 M;
`PestConfig::resolve_this_type` 4.3 M; `PestFacts::of` 3.9 M; vendor `lexically_normalized`
3.4 M; manifest `lexically_normalized` 2.5 M; `PackageIndex::owner_of_dir` 2.8 M; `Walk::run`
1.6 M; `Walk::add_file` 1.3 M; `Tree::node` 1.0 M; `Resolver::listed` 1.0 M;
`compare_components` 1.0 M; `PathBuf::pop` 0.9 M. At the base `compare_components` was 30.3 M
(item 5 removed it) and `Components::next` 99.9 M; HEAD has 96.2 M.

**Table 5: thread 11, `discover_vendor_context`** (393.1 M inclusive, 317 K self).
`parse_generated_file` 163.0 M (the generated autoload files; the thread also holds
`Utf8Chunks` 12.7 M and `LineIndex::new` 25.0 M, both under `parse_generated_file`'s
`Lexer::build`, Table 10); `dedup` 118.0 M (all of it the `drop_files_under_dirs` retain
closure, 112.7 M, plus `keep_first` 4.6 M); `Resolver::path` 98.3 M (101.4 M inclusive in the
thread's total; `listed` 30.7 M, `starts_with` 25.2 M, `append_concat` 14.2 M,
`lexically_normalized` 11.5 M, `entry_canonical` 8.4 M, `parent` 5.4 M, `file_name` 3.4 M);
`collect_dirs` 3.1 M. The retain closure's callees: `_strip_prefix` 27.7 M, `listable_dir`
24.2 M, `Path::hash` 23.2 M, `parent` 21.0 M, `symlink_metadata` 3.3 M, `extension` 3.1 M,
`PathBuf::push` 2.4 M. `DirListings::listing` is 52.6 M inclusive on this thread. The thread's
`Path::hash` self is 53.4 M and its `Components::next` self 35.9 M.

**Table 6: the main thread (01),** `Session::load` 424.1 M inclusive of 538.7 M (off), 537.2 M
of 651.7 M (warm). Off: `insert_numbered_file` 281.6 M (of which `drop_glue<[Stmt]>` 265.2 M:
the main thread frees the context files' trees), `Path::hash` 27.3 M, `_strip_prefix` 24.0 M,
`OnceLock<Listing>` init 21.6 M, `join_project_tables` 17.3 M, `join_decl_tables` 12.8 M,
`bind_file` 7.3 M, `Mutex<Worker>` drop 4.9 M, `split_decl_tables` 4.2 M. Warm: `bind_file`
401.0 M (of which the `OnceLock<FunctionReflection>` initialization is 393.1 M: the lazy
function signatures), `Path::hash` 27.3 M, `_strip_prefix` 24.0 M, `OnceLock<Listing>` 21.4 M,
`join_project_tables` 17.3 M, `insert_numbered_file` 14.0 M, `join_decl_tables` 8.8 M. Both are
serial work on the main thread, not added by items 1 to 3: R11 and R12 own them. Also on the
main thread: `judge_property_types` 16.7 M inclusive, `finalize_results` 11.4 M, `sort_by_key`
31.2 M, `apply_inline_ignores` 3.1 M, `Starts::of` 4.8 M.

**Table 7: row functions per thread, off HEAD (inclusive).** `Starts::reaching`: 02 5.9 M, 03
20.0 M, 04 5.6 M, 05 18.7 M, 06 6.2 M, 07 5.4 M, 08 5.0 M, 09 8.1 M (sum 74.9 M); `Walk::prunes`
2.9 + 2.2 + 2.3 + 1.7 + 2.0 + 1.7 M; `Walk::merge` 50.5 M on thread 02; `Tree::node` 43.5 M on
thread 02; `file_tables::bucket`/`merge_by_shard`: 03 426.9 M, 04 464.0 M (13.5 M self), 05
415.0 M, 07 208.8 M, 08 47.5 M, 09 406.4 M; `checker_for` 18.8 M on thread 06, 3.9 M on 08,
3.3 M on 09; `run_finishing_passes` 1,003.5 M on thread 05 (base 872.2 M; the +131.3 M is not
one of this plan's costs).

**Table 8: the ranges the ledger attributes.** Item 1, `428ed943` to `99b1c2e4`:
21,114,767,711 Ir, +226.2 M (path keys and their hashing about +128 M of self cost,
`canonicalize` +24.6 M, the content hash +25 M). The eight fixes, `99b1c2e4` to `ee56851c`:
20,909,227,687, −205.5 M. Items 2 and 3, `ee56851c` to `d870ddcb`: 21,289,129,115, +379.9 M
(`845efaab` +394 M, the other 27 commits −14 M). Item 5, `d870ddcb` to `aa896697`:
21,331,572,685, +42.4 M off, −35.6 M warm. Item 5's largest self differences (HEAD minus base,
off): `RefKey::hash` +31.4 M, `hash_one<&RefKey>` −25.7 M, `compare_components` −28.9 M,
`register_references` −16.6 M, `register_reference` +14.8 M, `Path::hash` +13.65 M, the
`merge_by_shard` closure +13.4 M, `HashMap<RefKey, HashSet<FileId>>::insert` +6.9 M,
`NameMap::insert` +6.3 M, `_strip_prefix` +5.7 M, `DeclTables::insert` +5.4 M, `RefKey` rehash
+3.6 M, `bind_file` +2.95 M. Warm: `from_utf8` −14.8 M, `Components::next` −19.2 M, `path_hash`
−7.8 M. Heap at one thread (`PHPSTAN_RS_THREADS=1`, dhat, off): HEAD allocates 2,936,855,495
bytes in 19,129,005 blocks (peak 809,609,428; exit 641,589,716 in 3,576,888 blocks); the base
2,913,109,581 in 19,175,695 (peak 807,381,174; exit 647,439,034 in 3,619,619);
`file_tables::bucket` +11.7 MB, `register_references` −10.46 MB, `register_reference`
+7.74 MB, `join_project_tables` +6.43 MB, `DuplicateClassSites::record` −5.73 MB.

**Table 9: the largest self costs at HEAD (off), for scale.** `Lexer::next_token` 835.1 M;
`memcpy` 634.4 M; `Lexer::next_tok` 582.3 M; `Parser::fill` 567.0 M; `Utf8Chunks::next`
532.5 M; `Lexer::next_raw` 490.5 M; `name_and_bits` 368.3 M; `is_folded_eq` 353.7 M; `mi_free`
341.3 M; `_mi_theap_malloc_zero` 314.9 M; `Path::hash` 194.3 M; `Components::next` 96.2 M;
`LineIndex::new` 156.7 M self. Inclusive: the parse job (`read_indexed`) 8,955 M, the check
(`check_files`' `run_checker`) 6,442 M, `parse_file` 8,023 M.

**Table 10: other readings.** Warm `crc32fast::hash` 33.5 M (26.1 M in `read_indexed`, where
each cached file's entry is checksummed on load; 7.8 M in the stub cache; "Not in this plan").
Warm `twox_hash` XXH3-128: `Hasher::write` 17.9 M, `finish_128` 2.4 M, 24.1 M in `read_indexed`
in all (the cache key; R9). Off-run `Utf8Chunks` callers on thread 11: only `LineIndex::new`
(12.7 M), from `Lexer::build` (25.0 M) in `parse_file`. Piwigo's `vendor/composer`:
`autoload_static.php` 1,808,580 bytes, `autoload_classmap.php` 1,620,829 (not parsed while a
static file exists), `installed.php` 94,049. Main-thread drops of `[Stmt]` (off run):
`insert_numbered_file` 265.2 M, the `build_generated_stub_data` closure 76.3 M (a stub thread),
`discover_vendor_context` 9.9 M. Walk hashing by caller: the `Starts::of` tables 2.0 M + 2.1 M
(R3), `listable_dir`'s `HashMap<PathBuf,bool>` rehash 1.5 M (R6), `keep_first` 4.5 M + `dedup`
0.2 M + rehash 2.2 M (R6).

## Appendix: how the numbers were made

The scripts are in `scripts/` at the repository root, and each gives its usage at its top. Run
the Python ones as `python3 -I scripts/<name>.py`; the paths written in the two shell scripts
are those of the 2026-10-17 runs. The binaries and profiles of those runs are not kept.

**Rules for every run.** One heavy job at a time; `nice -n 19`; the first three fields of
`/proc/loadavg` logged before each; the machine and the other sessions checked first
(`ps -eo ni,pcpu,comm --sort=-pcpu | head`); no `timeout`; outputs under
`~/.cache/prs-profile/<run>/`, never `/tmp`; kill by pid; builds in a `CARGO_TARGET_DIR` of
their own under the run's directory, so the shared checkout's `target/` (and another session's
builds) are not disturbed.

**A. Rebuild the profiling binary.** From a checkout at `aa896697`'s code (HEAD while only
documentation follows it):
`RUSTFLAGS="-C force-frame-pointers=yes" CARGO_TARGET_DIR=<dir>/target nice -n 19 cargo build --profile dhat -p phpstan-rs --bin phpstan-rs`
(6.5 min at load 1.2); the binary is `<dir>/target/dhat/phpstan-rs`. The heap variant adds
`--features dhat-heap`. A base build needs a worktree at `d870ddcb` with `vendor/phpstan-src`
copied in (never `--recursive`), its own target dir, removed afterwards.

**B. The callgrind and heap runs** (from the `regress-item5` harness, `run-all.sh`;
`PIW=/home/torres/piwigo17-frozen`,
`CHECK="check-project --config phpstan-rs.toml --format json"`,
`SCOPE="systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=1G nice -n 19"`):

- off-mode callgrind:
  `cd $PIW && env PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1 $SCOPE valgrind --tool=callgrind --callgrind-out-file=<dir>/cg.out --separate-threads=yes <binary> $CHECK > <dir>/out.json 2> <dir>/stderr.txt`;
- warm callgrind: first `env PHPSTAN_RS_CACHE_DIR=<cache dir> $SCOPE <binary> $CHECK` (fills
  the caches; not measured), then the same under valgrind with the same
  `PHPSTAN_RS_CACHE_DIR`; afterwards `find <cache dir> -type f -newer <fill exit file> | wc -l`
  must be 0;
- heap:
  `env PHPSTAN_RS_DHAT_UNTRIMMED=1 PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1 PHPSTAN_RS_THREADS=1 $SCOPE <heap binary> $CHECK`
  (writes `dhat-heap.json` in the working directory). The converters `convert.py`, `heap.py` and `regress.py` are not reproduced: a
  heap comparison needs them written again, and the dhat totals above are recorded;
- log each step with `date -Is` and the first three fields of `/proc/loadavg`; abort if
  `$PIW/dhat-heap.json` exists before a heap run.

**C. The callgrind scripts:** `scripts/callers.py` and `scripts/cgfn.py`.

- `callers.py <run dir> <regex> [n]`: the inclusive Ir of calls from each caller to the callees
  matching the regex, summed over a run's `cg.out-*` files.
- `cgfn.py <run dir> [<run dir> [n]]`: the run's total and per-thread Ir, and the largest self
  and inclusive costs; with a second run, the difference by function name.
  - Each file's `summary:` line must equal the sum of its self costs, which checks the parse.
  - A cost line right after `calls=` is the call's inclusive cost, charged to the caller's
    inclusive total only.
  - For one thread's file, import `parse(path)` from it; it returns the total, the self costs
    and the inclusive costs.

The per-thread tables and callee edges above came from `cgfn.parse` per file, plus a
callee-edge variant of `callers.py` that filters on the caller (`fn`) instead of the callee
(`cfn`).

**D. The wall-time tooling** (Step 0). Runs are in `/home/torres/piwigo17-frozen` with
`CHECK="check-project --config phpstan-rs.toml --format json"`. Off mode is
`PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1`; warm mode is
`PHPSTAN_RS_CACHE_DIR=<cache dir>`, filled by one run first (`touch <marker>` after it), and
`find <cache dir> -type f -newer <marker> | wc -l` must stay 0. Every run's stdout must have
the oracle's sha256.

- **To run Step 0 again:**
  1. The timeline: rebuild the profiling binary (A), record an off and a warm run under perf
     (below), and read the dumps with `scripts/perfdump.py`: the first and last sample of
     every thread (`threads`), cores busy and the role mix per window (`phases`, 0.1 s off
     and 0.05 s warm), the unused capacity per stretch (`unused`), the main thread's order
     (`main`), and the samples of each row's code (`incl`, `leaf`, `above`, `split`).
  2. The scaling curve: a `gate` build, `scripts/scaling.sh` and `scripts/fit.py` (below),
     with the CPU time read beside the wall time: the fit's form holds only while the CPU
     time stays the same as threads are added.
- **Build:** the `dhat` binary of A for perf; for timing,
  `CARGO_TARGET_DIR=<dir>/target-gate nice -n 19 cargo build --profile gate -p phpstan-rs --bin phpstan-rs`
  (6 min 16 s cold).
- **A row against the one before it:**
  `nice -n 19 python3 -I scripts/abab.py <out dir> 20 <off|warm> <name A> <name B>`, where a
  name is a directory of `.corpus-runs/gate/bin` holding `gate/phpstan-rs`. It runs 4
  unrecorded runs (A B A B; in warm mode they also fill each binary's own cache dir under
  `<out dir>`), then the pairs, and stops at the first output that is not the oracle's. It
  writes `times.json` (wall, peak RSS and CPU time per run) and `run.log` (the load at the
  start and the end), and prints the medians, the paired median and mean of B − A, how many
  pairs B lost, and the Wilcoxon W+, z and p, for wall time, peak RSS and CPU time.
- **Record,** after one unrecorded run:
  `nice -n 19 perf record -o <dir>/perf.data -F 499 -g -- env <mode> <dhat binary> $CHECK > <dir>/out.json 2> <dir>/perf-stderr.txt`;
  then `perf script -i <dir>/perf.data --no-inline -F tid,time,ip,sym > <dir>/<name>.script`
  (with the stacks, so no `-G`; about 1 s; 68 MB off, 51 MB warm). Before and after each run,
  `grep -E '^(compact|pgmigrate|thp)_' /proc/vmstat` (system-wide counters).
- **System calls:**
  `nice -n 19 strace -f -c -w -o <dir>/strace-summary.txt env <mode> <binary> $CHECK > <dir>/strace-out.json`
  (exact counts; `-w` gives each call's wall time, inflated by ptrace);
  `strace -ff -e trace=readlink,statx -o <dir>/st env <mode> <binary> $CHECK` writes one file per
  thread (`st.<tid>`), which splits the counts by thread (R14).
- **Vendor discovery alone:** `scripts/probe-vendor/`, a crate that calls
  `discover_vendor_context` with a fresh `DirListings` per run.
  - It is not a workspace member: its `Cargo.toml` has a `[workspace]` of its own, and its
    dependency on the `composer` crate is a path into the main checkout.
  - Build a copy outside the repository: copy the directory to a run directory under
    `~/.cache/prs-profile/`, copy the workspace's `Cargo.lock` into the copy, and build there
    with `nice -n 19 env CARGO_TARGET_DIR=<probe dir>/target cargo build --release --offline`
    (18.5 s).
  - Run `nice -n 19 <probe dir>/target/release/probe-vendor /home/torres/piwigo17-frozen 7`.
  - On 2026-10-17 the runs took 133.5, 127.2, 122.9, 104.6, 109.5, 103.5 and 108.9 ms, each
    with 1,744 files and 249 dirs.
- **mimalloc:**
  - `scripts/mimalloc-runs.sh` runs mimalloc's statistics at exit with and without purging,
    then two hyperfine rounds in alternating order, then one perf recording of each with the
    `/proc/vmstat` counters around it. It takes about 2 minutes. `GATE` is the gate binary and
    `CG` the `dhat` frame-pointer binary.
  - `python3 -I scripts/hfpool.py PURGE_DELAY=-1 <dir>/hf1.json <dir>/hf2.json` pools the
    rounds. It splits both `--export-json` files' runs by whether the command contains the
    marker, pools their `times`, and gives a two-sided Mann-Whitney U in the normal
    approximation.
  - `scripts/thp-probe.py` is run as
    `nice -n 19 python3 -I scripts/thp-probe.py <gate binary> <out.json> [MIMALLOC_ALLOW_THP=0]`,
    twice each, alternating.
  - mimalloc's options are documented in `c_src/mimalloc/v3/include/mimalloc.h` (the
    `mi_option_e` enum, in index order) and `src/options.c` (the defaults).
- **CPU growth with the thread count:**
  - Record the off run with `PHPSTAN_RS_THREADS=1` and then with `=8`, one right after the
    other (`perf record -F 499 -g` as above), and dump both.
  - Then `python3 -I scripts/cpugrowth.py <1-thread dump> <8-thread dump>`. It imports
    `perfdump.py` from its own directory.
  - Counts:
    `nice -n 19 perf stat -r 3 -e task-clock,context-switches,cpu-migrations,page-faults -o <file> -- env <off mode> PHPSTAN_RS_THREADS=<n> <gate binary> $CHECK > <out>`,
    for n = 1 and 8. With `-r 3` the output is written three times into `<out>`: check each
    171,439-byte third.
  - `cycles` and `instructions` print `<not supported>` on this VM, and
    `/sys/devices/system/cpu/cpu*/cpufreq` does not exist.
  - Check `ps` before each recording: a heavy job at normal priority beside the `nice -n 19`
    run distorts it, as it did the 20 s one-thread recording.
- **Step 0's tables from the dumps,** with `P="python3 -I scripts/perfdump.py"`. Checked on
  2026-10-17: `perfdump.py`'s commands give the same output as the ten scripts Step 0 used,
  byte for byte on both dumps, and `threads` and `kernel` give the numbers the ad hoc readings
  gave.
  - the threads and their tids: `$P threads <dump>`;
  - the timelines: `$P phases off.script 75945 0.1` and `$P phases warm.script 79694 0.05`; the
    main thread's order: `$P main <dump> <main tid> 0 2.1`;
  - the unused capacity: `$P unused off.script 0,0.21,0.90,1.50` and
    `$P unused warm.script 0,0.15,0.55,0.70,0.75,1.15`;
  - the rows' samples:
    `$P incl <dump> discover_vendor_context parse_generated_file drop_files_under_dirs 'Resolver>::path' bind_file insert_numbered_file 'Session>::load' 'Starts>::reaching' 'Walk>::merge' 'Tree>::node' walk::path_hash 'Walk>::prunes' checker_for judge_property_types finalize_results apply_inline_ignores`
    (a marker is a substring: `'Walk>::add'` also counts `add_file`); R1: `$P above <dump> Utf8Chunks 1` (the
    leaves right under `read_indexed`); R6: `$P split <dump> drop_files_under_dirs statx` and
    `$P split <dump> 'Resolver>::path' listed`; R11:
    `$P split off.script insert_numbered_file Stmt mentions_phpstan`; the merge:
    `$P mark off.script merge_by_shard`; the stub base: `$P mark off.script 'Stubs>::base' 8 100`;
  - "Ir is not time":
    `$P leaf off.script _mi_theap_malloc_zero '^<syntax::lexer::Lexer>::name_and_bits$' '^mi_free$' __memmove_avx_unaligned_erms '^<syntax::lexer::Lexer>::next_token$' 'Parser>::fill' Utf8Chunks 'Components as core::iter::traits::iterator::Iterator>::next' 'Path as core::hash::Hash>::hash'`
    (looser patterns count other leaves: `name_and_bits` alone also matches a closure's rehash,
    `next_token` the phpdoc lexer's);
  - kernel time: `$P kernel <dump> 0 2.1`, and `$P kernel off.script 0.8 1.5` for the
    finishing stretch.
- **Scaling:** `scripts/scaling.sh` runs 5 runs per thread count, in both modes. `W` is the run
  directory and `BIN` the gate binary, and the warm cache dir is filled first.
  - Then `python3 -I scripts/fit.py <dir>/scaling-off.json <dir>/scaling-warm.json` prints the
    Scaling table and the fits.
  - The script run on 2026-10-17 had no hash check, though its header said it did, so the gate
    binary's output was not checked that day. Every run exited 0, since hyperfine fails a
    command that exits non-zero.
  - `scripts/scaling.sh` has the check: the `ORACLE` line and the three lines after
    `hyperfine $mode exit`.
