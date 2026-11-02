# Status

**Phase 1: the repo's skeleton and process.**

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

## Scoreboard

Not built yet (Phase 2).

## Cost

Not built yet (Phase 2).

## Next

1. CI green on all four legs.
2. The release workflow: the five targets, with Linux binaries built for an older glibc or for musl.
3. Phase 2: `docs/design/scoreboard.md`, then the extractors and `cargo xtask parity`.
4. Phase 2: `docs/design/cost-meter.md`, then `cargo xtask cost` and its CI gate.
5. Run the legacy binary through the suites it can drive, for the baseline line.

## Later
