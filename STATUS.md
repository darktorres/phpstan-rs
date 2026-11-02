# Status

**Phase 2: the scoreboard and the cost meter.** Phase 1 is done.

## Done

- **Phase 0:** the legacy code is frozen.
  - The branches are `legacy/*`, read-only through a GitHub ruleset, and the tags are `*-final`.
  - Its findings are in `RESTART_FINDINGS.md` at `legacy-final`, and its lessons in `LESSONS.md`.
  - Its data outside git is in `~/.cache/phpstan-rs-legacy/` (see `LOST.md` there).
  - The legacy binary is at `~/.cache/phpstan-rs-legacy/bin/phpstan-rs-legacy-final`.
- `vendor/phpstan-src` is pinned to 2.3.4, the latest release, frozen. The 2.3.4 phar is pinned by digest in `parity/pins.toml`.
- The workspace has 14 empty crates in PHPStan's layout, plus the xtask.
- The CLI runs on its own thread, and a closed pipe exits with 141.
- `parity/port-map.tsv` lists 2,032 PHPStan files, all `todo`.
- **CI** runs on every push, and is green: fmt, clippy, doc and the port-map check; tests on Linux x86_64, Linux aarch64, macOS and Windows; tests in release; `cargo deny` and `machete`.
- **The release workflow** builds five targets with checksums. The Linux binaries are static musl builds. A dry run is green on all five.

## Scoreboard

Not built yet (Phase 2). The rules and types extractors run (`parity/extract/`):
- rules: 3,548 cases (3,544 `analyse`, 4 `fix`);
- types: 21,299 asserts from 1,986 files, every expected value resolved.

## Cost

Not built yet (Phase 2).

## Next

1. `docs/design/scoreboard.md`: the extractors, the oracle php pinned for CI, and the comparison per suite.
2. `cargo xtask parity`, with the lexer and parser suites first; their oracles need no port code.
3. `docs/design/cost-meter.md`, then `cargo xtask cost` and its CI gate.
4. The level-oracle and end-to-end extractors; the rules and types baselines recorded.
5. Run the legacy binary through the suites it can drive, for the baseline line.

## Later

- The types suite's three `processFile` classes (`LegacyNodeScopeResolverTest`, `Generics\AnalysisContextTest`, `ScopePhpVersionTest`) assert in scope callbacks and are not extracted yet.
