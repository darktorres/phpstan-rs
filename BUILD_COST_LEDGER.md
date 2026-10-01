# Build-cost work: what is left

What was done, and its measurements, is in section 12 of
`PROJECT_BUILD_COST_INVESTIGATION.md` and in the git history. This file keeps
only what a later change still has to know or do.

## Decisions and caveats

- **Extensions are all loaded (#8).** Loading only the enabled phpstorm
  extensions would make code using an undeclared one report
  `class.notFound`. Loading all matches PHPStan, and unreferenced extension
  classes cost nothing per project since the stub cache, the lazy entries
  and the shared base.
- **`Project::built_once`** only gates a `debug_assert`; the shared base
  always keeps raw declarations, and `end_declaration_changes`' `keep_raw`
  matters only for the base build. Dropping a built-once project's raw
  declarations after its finish was tried: peak memory on Piwigo is 2.17 GB
  either way, and the clones cost 0.35% of the run, so it stays as it is.
  The peak is elsewhere (the parsed files and the types of the checks).

- **The worker pool is not sized by workload.** A one-file run takes about
  the same on 1 thread and on 8 (47.4 and 47.0 ms over 15 runs of `perf stat`;
  three alternating rounds of `hyperfine` differ by under 2 ms, within the
  noise), so the cost of idle threads is not worth plumbing a size through
  discovery. An earlier reading of 9 ms came from one 8-thread run that was
  last in a sequence while the load changed.

## Gates

`cargo xtask gates` (the quick tier) passes at the end of this work. Four
gates had been failing before it - the test manifest, the fuzz crate,
`cargo deny` and the module graph - and were fixed on this branch; master has
the same four failures, so merging brings these fixes along. Not run here:
the `--full` tier's feature matrix, `machete` and the performance gate.

## Investigated and declined

Each was measured from the profiles of the Piwigo corpus run (8 threads, 4.6 s
at the end of this work), so a later profile can be compared with them.

- **Parallelize the sequential insert of loaded files.** It is a 0.5 s serial
  stretch (11% of the run) after the files are parsed: `register_references`
  30% of it, `closure_scope` 19%, the load of the registry with PHPStan's
  source 12%, `insert_built_decl` 8%, `PackageIndex::owner` 6%. Only a large
  first load pays it, never an edit. Overlapping it with the parse means
  inserting files as they finish parsing, which conflicts with choosing the
  registry before the first insertion (it needs every file's mention of
  PHPStan; a textual scan would do) and moves the borrow of the project
  between the parse tasks and the inserting loop. Worth about 0.4 s of 4.6 s
  on a corpus of fourteen thousand files; revisit if load time on such
  corpora matters.
- **Name interning and `Name::lower`.** `Interner::name` is 4.75% inclusive and
  about 80 ns a call (a hash, a shard lock, a weak-pointer upgrade);
  `Name::lower` is a cached field read behind two pointers, and its 1.5-4.8%
  is the memory latency of that indirection on the hottest lookup path, which
  a cached hash can't remove because a table is looked up by `&str`.
- **The purity walks.** `resolve_transitive_purity` is 2.8% of the corpus run;
  its classification is parallel since the serial-stretch work, what is left
  are two fixed-point loops over declarations.
- **Fused finishing passes, a patched rayon sleep threshold.** The scheduler
  is 6.4% of the corpus profile, 5.3% of it the kernel's `finish_task_switch`
  (threads waking after file reads, locks and parking); rayon's own spinning
  and stealing is under 0.1%. A one-file run takes the same on 1 thread and on
  8 (47.4 and 47.0 ms), so the pool's size is not worth tuning either.
