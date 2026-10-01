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

## Follow-ups, each a few percent at most

- Parallelize the sequential insert of loaded files, mainly
  `register_references` (about 5% of the Piwigo run).
- Left under the last profile, each under 3% of a run: name interning,
  `Name::lower`, the purity walks.
- Fused finishing passes and a patched rayon sleep threshold: the data shows
  spinning is a small share, so only if a profile ever points there.
