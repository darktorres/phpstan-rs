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

`cargo xtask parity` runs the lexer, parser, types and rules suites; `parity/score.json` holds the totals, and CI checks it against a full run under the pinned `php:8.5.4-cli` image on every push. Nothing is ported yet, so every case fails.

```
lexer 0/39858 parser 0/13286 types 0/21299 rules 0/3609
```

## Cost

Not built yet (Phase 2).

## Next

1. `docs/design/cost-meter.md`, then `cargo xtask cost` and its CI gate.
2. The parser suite's other inputs (nikic's fixtures, php-src's `.phpt` files, the operator pairs), pinned.
3. The phpdoc suite.
4. The level-oracle and end-to-end suites; then the legacy binary's baseline line.
5. `cargo xtask parity --diff <commit>`, classifying each changed case.

## Later

- The types suite's three `processFile` classes (`LegacyNodeScopeResolverTest`, `Generics\AnalysisContextTest`, `ScopePhpVersionTest`) assert in scope callbacks and are not extracted yet.
