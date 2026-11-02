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

`cargo xtask parity` runs the lexer, parser, phpdoc, types and rules suites; `parity/score.json` holds the totals, and CI checks it against a full run under the pinned `php:8.5.4-cli` image on every push. Nothing is ported yet, so every case fails. The lexer and parser read phpstan-src's `tests/` and `vendor/`, nikic's parser fixtures (284 sections), php-src 8.5.4's `.phpt` FILE sections (21,445) and 14,142 generated operator pairs; nikic and php-src are fetched by their pins. The phpdoc suite parses every distinct doc comment in them, and the inputs of phpdoc-parser's own parser tests.

```
lexer 0/147471 parser 0/49157 phpdoc 0/41908 types 0/21299 rules 0/3609
```

Where the legacy binary (`legacy-final`) stood, measured once through what its command line can do:
- **types:** 8,125 of 20,188 `assertType` cases (40.2%). It has no `assertNativeType`, `assertSuperType` or `assertVariableCertainty`, so 8,125 of 21,299 overall. An assert counts as passing when its file was checked and no mismatch was reported on its line, which overcounts what it never evaluated.
- **parser:** it has no nikic-shaped AST, so only its parse verdicts compare. Over phpstan-src's `tests/` and `vendor/`, they agree on 13,278 of 13,286 files, with no false refusals; the 8 others are files PHPStan rejects and it accepts (4 `NameResolver` errors, 4 compile-time errors such as a `void` parameter).
- **lexer, phpdoc, rules:** its command line can print no tokens or PHPDoc trees, and run no single rule.

## Cost

`cargo xtask cost` measures the workloads of `perf/workloads.toml` under callgrind and DHAT. CI's cost gate compares each push with its base commit, and was shown to fail on a planted regression. `perf/history.tsv` holds the rows recorded on this machine:

| workload | | Ir | blocks | bytes | peak | live |
|---|---|---|---|---|---|---|
| startup | 75a4b7944 | 312,225 | 27 | 4,661 | 2,236 | 548 |
| startup | legacy-final | 2,108,536 | - | - | - | - |
| parse-src-nsrt | 75a4b7944 | 313,556 | 29 | 4,762 | 2,289 | 548 |
| parse-src-nsrt | legacy-final (`check`) | 6,406,641,908 | - | - | - | - |

The legacy's heap is not measurable this way (it allocates through mimalloc), and its Ir varies by about 0.24% between runs. The port's `parse-src-nsrt` row measures the unported answer until M1.

## Next

1. The level-oracle and end-to-end suites.
2. `cargo xtask parity --diff <commit>`, classifying each changed case.
3. M1's design doc (`docs/design/syntax.md`), once the Phase 2 exit holds.

## Later

- The types suite's three `processFile` classes (`LegacyNodeScopeResolverTest`, `Generics\AnalysisContextTest`, `ScopePhpVersionTest`) assert in scope callbacks and are not extracted yet.
