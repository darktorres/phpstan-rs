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

## Gates failing before this work, still failing

- Test manifest: renames in master's history before the merge-base, without
  `gates/test-renames/` entries.
- Fuzz crate: `project::parse_file` takes bytes.
- `cargo deny`: yanked `yoke-derive`.
- Module graph: 14 edges (and 5 allow lines for edges that no longer exist).

## Follow-ups, each a few percent at most

- Size the worker pool by workload: a one-file run takes 54 ms on 8 threads
  and 45 ms on 1-4 (the file count isn't known before the pool exists).
- Parallelize the sequential insert of loaded files, mainly
  `register_references` (about 5% of the Piwigo run).
- Left under the last profile, each under 3% of a run: name interning,
  `Name::lower`, the purity walks.
- Fused finishing passes and a patched rayon sleep threshold: the data shows
  spinning is a small share, so only if a profile ever points there.
