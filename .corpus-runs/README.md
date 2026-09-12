# Piwigo corpus-run snapshots

Gitignored (large, environment-specific, not source) - lives here rather
than a session scratchpad so it survives across sessions and never needs
rebuilding an old commit just to re-diff against it.

## Layout

    .corpus-runs/scoped/<commit-hash>.txt    - phpstan-rs.toml-scoped run
    .corpus-runs/unscoped/<commit-hash>.txt  - --no-composer unscoped run

`<commit-hash>` is the short hash of the phpstan-rs commit the binary was
built from when the run was taken (`git rev-parse --short HEAD`). Piwigo's
own checkout (`/home/torres/piwigo17-frozen`) is treated as fixed/frozen,
so it isn't part of the key.

## Producing a run

    cargo build --release --bin phpstan-rs
    HASH=$(git rev-parse --short HEAD)
    cd /home/torres/piwigo17-frozen
    /home/torres/projects/phpstan-rs/target/release/phpstan-rs check-project \
        --config phpstan-rs.toml 2>&1 | LC_ALL=C sort \
        > /home/torres/projects/phpstan-rs/.corpus-runs/scoped/$HASH.txt
    /home/torres/projects/phpstan-rs/target/release/phpstan-rs check-project \
        --no-composer . 2>&1 | LC_ALL=C sort \
        > /home/torres/projects/phpstan-rs/.corpus-runs/unscoped/$HASH.txt

Only save a run for a real commit (working-tree/uncommitted state has no
stable key to file it under - re-run once committed instead).

## Diffing

Before rebuilding an old commit to get a comparison snapshot, check here
first - `ls .corpus-runs/scoped/` - the commit you want may already have
one. Only fall back to a worktree rebuild when it's genuinely missing.

    diff .corpus-runs/scoped/<old-hash>.txt .corpus-runs/scoped/<new-hash>.txt

## Known snapshots (as of 2026-09-11)

- `264d910` - last snapshot before the 0247b04/d1d1d61 phase-design batch
  (355 scoped diagnostics) - no unscoped run exists for this one.
- `d1d1d61` - post-batch, **before** the use_map/throws regressions were
  fixed (5012 scoped diagnostics) - kept deliberately as the historical
  record of that regression, not a state to build against.
- `c522416` - post-fix (359 scoped / 40503 unscoped).
- `1a3f3e4` - master2 branch tip before its rebase onto master (358
  scoped, 0 skipped) - the bucketed-findings baseline the 3 false-
  positive fixes below were diffed against; produced/stored in the
  `phpstan-rs-master2` worktree, not the canonical one.
- `b33d307` - master2 rebased onto master, **before** the 3 false-
  positive fixes (353 scoped) - the clean "pre-fix" comparison point
  isolating just those 3 commits' effect from the unrelated commits the
  rebase itself pulled in; produced in a throwaway worktree at this
  commit, stored under `phpstan-rs-master2`.
- `299e5c5` - post-fix (341 scoped). Removed exactly the 12 confirmed
  false positives (2 `staticMethod.resultUnused` on a bare
  `parent::__construct()`, 8 `method.unused`/`property.onlyWritten`/
  `classConstant.unused` from private-member usage inside a closure/
  arrow-fn, 2 more of the same from usage through a non-`$this`
  same-class-instance local variable) and nothing else in the diff
  moved - produced/stored in the `phpstan-rs-master2` worktree.
- `452eb2b` - post Phase-1 of the corpus-findings-triage plan (339
  scoped). Removed exactly 2 `argument.type` findings
  (`PaginationService::createNavigationBar`'s own bare `min()`,
  `HistoryService::logVisit`'s own `array_find()`) and nothing else
  moved, despite `min`/`max`/`array_find`-family/non-variable-`++`/`--`
  each touching 20-36 more call sites in `src/` alone - produced/stored
  in the `phpstan-rs-master2` worktree.
- `069cff9` - post `#[ArrayShape(...)]` attribute support (348 scoped).
  Removed `ImageDerivativeControllerTest.php`'s own `idcPixel()`/
  `imagecolorsforindex()` `return.type` finding; correctly *added* one
  new, genuine `nullCoalesce.offset` finding at `ImageBackend::
  getGraphicsLibrary` (`gd_info()['GD Version'] ?? null` -
  `gd_info()`'s own `#[ArrayShape]` declares that key unconditionally
  present, matching what real PHPStan would also now report with the
  same precision) - produced/stored in the `phpstan-rs-master2`
  worktree.
- `e9bde62` - post the CategoryService `mixed`-leak fix (three layers:
  empty-shape dynamic-key reads, `check_expr`'s own `Index`-read arm
  unified with `index_result_type`, `advance_incdec_type`'s own `Union`
  recursion) (360 scoped) - current baseline for future diffs. Net +12
  from `069cff9`, all individually investigated (see ROADMAP.md's
  "Known but not yet scheduled"/"Permanent design boundaries" - a
  by-reference-`foreach`-mutation gap and the cross-variable-
  correlation boundary, both newly cross-referenced from this run) -
  1 genuine new true positive
  (`UserService::searchCaseUsername`), 1 confirmed false positive from
  that separate by-ref-foreach gap, 10 sound-but-imprecise cross-
  variable-correlation findings. Produced/stored in the
  `phpstan-rs-master2` worktree.
