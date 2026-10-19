# The code scoped to Linux: the cross-platform fixes

## State (2026-10-18)

- **The plan is approved** (2026-10-18), with the decisions below. `WORK_STACK.md`'s level 9
  tracks it.
- **Built:** R0 (`4f6bc51a`), R1 (`ce593a47`), R2 (`d8de9530`), R3 (`d360a4df`), R4
  (`c6902324`), R5 (`52844b07`), R18 (eight commits, `0e908059` to `80b3bbd9`), R5's
  round-2 follow-up (`8d81da46`), R6 (`2526323b`, `3350adbd`), R6's corrections (`daca16d5`),
  R5's second follow-up (`7a757a8e`), R7 (`1146b4f6`, `1fe8c8e4`), R8 (`7d5bddd4`, `06486da9`),
  R9 (`da9e86df`), R10 (`007d827b`), R11 (`a94e27ec`), R12 (`acc33609`), R13 (`f5e418d6`,
  `ee903695`, `2c7bf898`, `df4be250`), R14 (`92074e5a`, `4ce90ca9`, `08315491`) and R15
  (`7d7f2ce5`, `04a78a8b`, `b2528e33`), round 3's four fixes (`bdc49385`, `ccb312b4`,
  `9e1f8ec9`, `e45a69c0`) and R16 (`9c555663`). The rows are built in the order of the table
  below, one commit each, with a pathspec, since another session shares the checkout. R18 has one
  commit for each step, R6 has a second for the rule its build found, R7 has a second for the
  case rule, R8 has a second for the files an environment change re-checks, R13 has one for
  the fixture harness, one for four tests' paths, one for the links and one for the line
  breaks, R14 has one for the embedded data, one for two debug-only items and one for the heap
  profiler, R15 has one for the compiler flags, one for xtask and one for the README and
  the scripts, and round 3 has one fix for each cause.
- **Round 1** runs `d8de9530`, pushed on 2026-10-18 with the user's go-ahead.
  - Extension CI passed on all three systems. The Windows round trip took 1179 ms. At
    `120c990f` it had failed 10 s into the test, waiting for the extension to activate or its
    client to run, before the 30 s wait for diagnostics began. Two things changed between the
    runs: the LF checkout and 223 commits.
  - Rust CI passed on aarch64 Linux and failed on macOS, Windows and the gates job. Each failure
    is classed in R2's "Round 1's result", with the rows it extends: R5, R6, R7, R13 and R15.
  - The gates job failed at the module graph, which predates this plan. Since `54e7e02d`,
    each gates check runs whatever the one before it gave.
- **Round 2** runs `52844b07`, pushed on 2026-10-18 with the user's go-ahead.
  - Extension CI passed on all three systems. The round trip took 1233 ms on Windows and
    2628 ms on macOS.
  - Rust CI passed on aarch64 Linux. It failed on macOS (5 tests, fixtures included), on
    Windows (31 tests and fixtures, clippy and dhat) and on the gates job. Each failure is
    classed in R2's "Round 2's result", with the rows it extends: R5 (a follow-up), R13, R14
    and R18.
- **Round 3** runs `c5bec58c`, pushed on 2026-10-18 with the CI split of the other session.
  - Extension CI passed on all three systems. The round trip took 1240 ms on Windows, 1786 ms
    on macOS and 747 ms on Linux.
  - Rust CI passed on x86_64 and aarch64 Linux, and every platform's clippy and heap profiler
    build passed. It failed on the lint job (3 steps), macOS (1 test) and Windows (1 lib test,
    `path_bytes`' two mixes and 9 fixtures). Each failure is classed in R2's "Round 3's
    result", with the rows it extends: R6, R8, R10, R14 and R16.
- **Round 4** runs `9c555663`, pushed on 2026-10-18 with the user's go-ahead: round 3's fixes,
  R16 and its bench step on the three platform legs.
- **The module graph is fixed in this batch** (the user's decision with round 2's push): R18,
  built before R6. The 20 forbidden edges are refactored away, not listed as exceptions, and
  the graph passes. Round 2 reached the gates job's later steps, and R18 fixes their four
  failures too.
- **The questions the build left were answered** (2026-10-18, decisions 5 to 8): the `\` rule
  and the JSON report's spelling, the disabled server, the SQLite path and the include rule.
  The user's instruction was to take the most proper design of each from what a run can know,
  with no weight on the size of the change or on what PHPStan does.
- **Next:** round 4's result, then round 5 with the end-of-batch checks, whose push is asked
  first. R17's records are written while round 4 runs.

## Context

I claimed that the project builds only for Linux, and used the claim to justify R7's `#[cfg(unix)]`
byte reads (`composer::path_bytes`). The user corrected it: "its not true that this project only
targets linux". The facts are these:

- `release.yml` builds five targets: x86_64 and aarch64 Linux, aarch64 and x86_64 macOS, and x86_64
  Windows (MSVC). Only the native legs are run (`--version`); the aarch64 Linux and x86_64 macOS
  binaries are cross-built and never run.
- `extension-ci.yml` builds the release binary and runs the VS Code smoke test on Ubuntu, macOS and
  Windows.
- `rust-ci.yml` (clippy, the tests, the fixtures) runs on `ubuntu-24.04` only. So code under
  `cfg(not(unix))` is never linted, and no test has ever run on Windows or macOS.
- This branch's last push (`120c990f`, 2026-10-14) failed in two places:
  - The Windows smoke test: "activates on a .php file and produces diagnostics via a real server
    round trip" got no diagnostics in 30 s.
  - The Rust CI stopped at "Lockfiles match the manifests". `fuzz/Cargo.lock` still fails
    `cargo metadata --locked` at HEAD.

The code, read at `791a57c6` (the same code as `8a1ca1e0`), is scoped to Linux in these ways.

- **Paths on Windows:**
  - `fs::canonicalize` returns `\\?\C:\…`, and every resolved path carries that prefix. In a
    verbatim path `/` is not a separator, so discovery's spelled paths
    (`__DIR__ . '/../pkg/X.php'`) name no file. `__DIR__` holds the prefix where PHP holds
    `C:\…`, and a path's URI encodes the prefix into its authority.
  - `path_hash` hashes a drive prefix by its spelling: `c:` and `C:` are equal `Path`s with
    different hashes.
  - The URI code drops a UNC URI's host, strips a drive letter on Unix too, and decodes a name's
    bytes lossily.
  - The report's paths are spelled with `\`, so an `ignoreErrors` path written with `/` never
    matches, and a baseline made on one platform does not match on the other.
- **PHP-visible values:**
  - The include rule takes `PATH_SEPARATOR` as `:`, splits include paths on `:`, and uses
    Debian's default include path.
  - On Windows, vendor `path_of` turns bytes that are not UTF-8 into the empty path.
- **Runtime:**
  - Windows gives the main thread 1 MiB of stack, which is no larger than the stack guard's
    1 MiB red zone.
  - On Windows the cache directory is off (`HOME` is unset), so every run builds the stubs
    cold.
  - The per-file cache's last-use refresh fails on Windows (a read-only handle), so eviction
    there goes oldest-written first.
  - Raw ANSI escapes reach a Windows console.
- **Tests and build:**
  - The tests:
    - The `cli` test target does not compile on Windows: its unix-only import has no `cfg`.
    - The link tests are `#[cfg(unix)]`.
    - There is no `.gitattributes`, so a Windows checkout embeds CRLF stubs.
  - The build scripts:
    - `stub-sources/build.rs` spells the embedded stub paths with `\` on Windows, against the
      checked-in `index.tsv`'s `/`.
    - The patched dhat's frame-pointer walk calls glibc's `pthread_getattr_np` on any x86_64
      target, so it cannot compile for macOS or Windows.
  - Developer tooling: `-Z threads` applies to x86_64 Linux only; xtask spawns `touch`, `diff`
    and GNU `time`; the scripts read `/proc`.
- **The records:** `path_bytes`' doc says nothing of the platforms, and LOAD-8.7's −126.7 M is a
  Linux measurement presented without the platform.

This plan fixes each case where it applies, in the most proper way, and changes nothing on Linux
except where Linux was wrong too: `uri_to_path`'s `/c:/` and lossy decoding, and the cache
directory where `HOME` is unset. The Piwigo oracle and the monolog output must stay byte-identical.

## Decisions (2026-10-18)

1. **The cache directories follow each platform's convention.** Linux keeps `$XDG_CACHE_HOME`
   or `~/.cache`, macOS uses `~/Library/Caches`, and Windows `%LOCALAPPDATA%`, through the
   `dirs` crate. `PHPSTAN_RS_CACHE_DIR` still overrides.
2. **`path_bytes` gets a Windows grammar too**: differential tests on the Windows leg, timed by a
   bench on the runner.
3. **The Windows and macOS runs are CI legs.** I push `callgrind-20g` when a round is ready,
   asking each time. The first push publishes the branch's 219 unpushed commits.
4. **The case rule is probed per root.** Matching against configured text folds case only where
   the root's directory does.
5. **A `\` in configured path text names a separator and a name's own `\`, and every machine
   format spells a file with `/`.**
   - Config files and baselines travel between platforms with the repository. Outside Windows
     a `\` in a path is ambiguous: a separator written on Windows, or a character a Unix name
     can hold. It names both. A `/` is a character of no platform's names, so it names only a
     separator.
   - JSON, checkstyle, JUnit and baselines are read by tools, so one project gives the same
     report on every platform: they spell a file with `/`. Text, friendly and the warnings on
     stderr are read by people, who paste the paths into a shell, so they keep the platform's
     spelling.
6. **A disabled server does nothing visible.** It clears every file it has published, runs no
   analysis on any trigger and answers every request with its empty result. It keeps its
   documents current, so enabling it again analyses what the editor holds. In the extension,
   `enable` starts and stops the client at once, `configPath` is applied live by the server
   and needs no restart, and `serverPath` alone restarts the client, since it names the
   process.
7. **A path the config file names is relative to the config file, and the analyser never
   creates or changes a database.** The SQLite reflection's `path` resolves through the one
   function that resolves every configured path, and opens read-only, with no create flag and
   no URI parsing. A wrong path then fails naming where it looked.
8. **An include is reported only when no run could find its file.** The analyser knows the
   project's files. The include path, the working directory and the platform are the
   deployment's, and the analysing machine's `/usr/share/php` says nothing of them. So the
   rule keeps every value a run can have, reads an include in a Unix world and in a Windows
   world, looks where PHP looks, in PHP's order, and reports a value only when no combination
   finds a file. R8 has the rest.

## How the rows are designed and judged

- **The most proper fix, not the smallest.**
  - One function per concern.
  - A lint enforces it where it can: `disallowed-methods` for `canonicalize`.
  - Each `cfg` branch is correct on its own platform, not merely compiling.
- **Linux is the control.** Every row leaves Linux output byte-identical unless the row names a
  Linux defect it fixes. The end of the batch proves it with the oracle, monolog and one
  callgrind run.
- **Windows and macOS are verified on the CI legs (R2).** macOS cannot be built here: aws-lc
  (rustls), the bundled SQLite, mimalloc, PCRE2 and stacker's assembly need the target's C
  toolchain and SDK. `x86_64-pc-windows-gnu` can: MinGW's `gcc` compiles them, and wine runs
  the tests on Windows' own `std::path` (`scripts/winrun.sh`, from R16 on). That is not the MSVC
  leg, which keeps the last word. Wine makes no junction or symlink and lists none as Windows
  does, so the link tests fail only there (composer's 5, and the package's 26 when last run).
- **No saving without a measurement.** R16's Windows reads are timed per call by a criterion
  bench on the runner. No load-level Windows figure can be made, and none is claimed.
- **Working rules:**
  - One commit per row, with a pathspec, since another session shares the checkout.
  - `cargo fmt` first, then check, `clippy -D warnings` and the row's targeted tests in the
    debug profile, with `TMPDIR` under `~/.cache`.
  - A push only after asking.
  - Comments carry no task references and no narrative.
  - A design question is answered from what a run can and cannot know, settled with php 8.5
    where it can be, and never from what PHPStan does or what the change costs. PHPStan is
    cited as prior art for a mechanism and not as the reason a design is right.

## The rows, in the order they are built

| | row | what | platforms | needs |
|---:|---|---|---|---|
| 0 | R0 | `fuzz/Cargo.lock` matches its manifest | CI | — |
| 1 | R1 | the deep-nesting CLI test runs everywhere | Windows | — |
| 2 | R2 | CI legs on macOS, Windows and aarch64 Linux; LF checkouts; the release's cross-built binaries run | all | R0, R1 |
| 3 | R3 | one canonical form of a path, without `\\?\` | Windows | R2 |
| 4 | R4 | equal paths hash alike | Windows | — |
| 5 | R5 | URIs ↔ paths on every platform | all | R3 |
| 6 | R18 | the gates job: every module has its crate, no edge breaks the layering, and the later steps pass | CI | R2 |
| 7 | R6 | machine reports spell a file with `/`; a `\` in configured path text names a separator and a name's own `\` | all | R3 |
| 8 | R7 | the case rule, probed per root; a `\` in a configured root; the SQLite path | all | R6 |
| 9 | R8 | the include rule: what a run can find | all | R3 |
| 10 | R9 | discovery: unreadable spellings, listing misses | Windows, macOS | R3 |
| 11 | R10 | the program runs on a thread whose stack it chooses | Windows, small `ulimit -s` | R1 |
| 12 | R11 | the caches where each platform keeps them, refreshed on Windows | Windows, macOS | — |
| 13 | R12 | colours on Windows consoles | Windows | — |
| 14 | R13 | the tests on every platform: links, CRLF, Unix spellings | all | R2, R3 |
| 15 | R14 | build scripts: the same embedded data everywhere; dhat's walk per platform | all | R2 |
| 16 | R15 | developer tooling | macOS, Windows | — |
| 17 | R16 | `path_bytes` reads Windows paths from their bytes | Windows | R3, R4, R18 |
| 18 | R17 | the records | — | all |

CI rounds, each push asked:

- round 1 after R2, the inventory;
- round 2 after R5, the smoke round trip;
- round 3 after R13 to R15;
- round 4 after R16, the bench;
- round 5 at the end, with the release dry run.

## Rows

Lines are as read at `791a57c6`.

### R0. `fuzz/Cargo.lock` matches its manifest

**Now.** `cargo metadata --locked --offline --manifest-path fuzz/Cargo.toml` fails ("cannot
update the lock file"). That step stopped this branch's last Rust CI run, so no test step has
run on CI since.

**Change.** Update the lock offline with the minimal change that `cargo metadata --offline`
makes, then check both lockfiles with `--locked`.

**Verify.** Both `--locked` commands of `rust-ci.yml`'s first step pass.

### R1. The deep-nesting CLI test runs on every platform

**Now.** `check_does_not_crash_on_deep_nesting_or_long_chains` (`tests/cli.rs:1351`) imports
`std::os::unix::process::ExitStatusExt` with no `cfg`, so the `cli` test target does not compile
on Windows. On Windows `cargo test --workspace --all-targets` fails before any test runs.

**Change.** Assert that the status is the one a finished `check` returns: 0, since `run_check`
(`main/check.rs:13`) returns nothing and never exits otherwise. The test then fails in each case
that matters:

- a crash, which ends in a signal on Unix (no code), and on Windows in an NTSTATUS code
  (0xC00000FD for a stack overflow, 0xC0000409 for a fast fail);
- a panic, which ends in 101.

The message keeps the status and the stderr.

**Test.** The test itself, here now and on the legs in round 1.

### R2. CI legs on every platform, LF checkouts, and every shipped binary run once

**Now.**

- `rust-ci.yml` runs on `ubuntu-24.04` alone.
- With no `.gitattributes`, a Windows runner, whose `core.autocrlf` is true, checks out CRLF.
  That covers the fixtures, `index.tsv`, phpstorm-stubs (tracked here) and the phpstan-src
  submodule's `.stub` files (its `.gitattributes` gives `eol=lf` to `*.php` only). The Windows
  release binary embeds CRLF stubs.
- `release.yml` runs only the native binaries. Its comments, and `extension-ci.yml`'s, say that
  the repository has no GitHub remote, which is no longer true.

**Change.**

The `platforms` job below is now three jobs, `platform-check`, `platform-test` and
`platform-fixtures`, on the same runners (`rust-ci.yml`, commit a9f1c05d).

- A `platforms` job in `rust-ci.yml`:
  - Matrix: `macos-26` (arm64), `windows-2025-vs2026` (x64) and `ubuntu-26.04-arm` (aarch64
    Linux, free for public repositories). This repository is public.
  - Checkout: LF (`git config --global core.autocrlf false` before checkout), the phpstan-src
    submodule, the pinned toolchain and the caches, as the `gates` job has them.
  - Steps, each run whatever the one before it gave (`if: ${{ !cancelled() }}`), so one failure
    hides no other:
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - `cargo test --workspace --all-targets --no-fail-fast`;
    - `cargo test --profile gate --test fixtures`;
    - R14's dhat check.
  - The `gates` job stays Linux's, with xtask, deny, machete and hack.
- `.gitattributes`: `* text=auto eol=lf`. The repository's own files check out LF everywhere.
  - The one file committed with CRLF on purpose, the fixture
    `the_single_newline_after_a_closing_tag_is_part_of_the_tag__1.php` (`?>\r\n`), is `-text`.
  - No binary type is listed: the repository tracks no binary file. The only files `text=auto`
    reads as binary are two gate approvals whose lone CRs it leaves as they are.
  - Renormalising changes no blob: every other tracked file is LF or has no line ending.
  - No tracked name is one a Windows or macOS checkout cannot hold: no reserved device name, no
    forbidden character, and no trailing dot or space, here or in the submodule. The longest
    path is 200 characters, 227 under the runner's `D:\a\phpstan-rs\phpstan-rs\`.
  - One pair of tracked directories differs only in case: `src/Http/` and `src/http/` in the
    fixture `psr4_locations_are_checked`. Round 1 found it. The first scan compared whole file
    paths and not their directories; the scan of every directory finds no other pair, here or in
    the submodule. R13 reshapes the fixture.
- `extension-ci.yml` and `release.yml`: every leg sets `core.autocrlf false` before checkout,
  since `.gitattributes` does not reach the submodule's 65 `.stub` files. The binary they build
  embeds Linux's bytes. The stale comments go.
- The toolchain cache key adds `runner.arch` in every workflow, since an x64 and an arm64 Linux
  leg now share `runner.os`.
- `release.yml`:
  - The aarch64 Linux leg builds natively on `ubuntu-26.04-arm` and runs its binary. This
    replaces the cross toolchain step. The image's glibc is 2.43, as `ubuntu-26.04`'s, and the
    README's note names both images.
  - The x86_64 macOS binary runs under Rosetta on the arm64 runner (`arch -x86_64`). The image's
    software list does not name Rosetta 2, so the leg installs it when `arch -x86_64
    /usr/bin/true` fails.
  - Every leg runs its packaged binary: `--version`, then `check-project --no-composer --format
    json` over `<?php strlen(42);`, which must report one `argument.type` diagnostic, from the
    embedded stubs.
  - The checksum uses `shasum -a 256` where `sha256sum` is missing: the macOS image has no GNU
    coreutils. The line is the same.
  - The pinned toolchain is installed by its own step, as in the other workflows. mold is set up
    on x86_64 Linux only, the one target `.cargo/config.toml` links with it.
- `extension-ci.yml`:
  - Its path filters add what the binary embeds (`vendor/phpstan-src`,
    `vendor/phpstorm-stubs/**`) and `.gitattributes`.
  - A failed smoke run prints the last VS Code session's logs. The language server's channel
    holds the server's stderr and, since the test sets `phpstan-rs.trace.server` to `verbose`,
    every message with its parameters. The smoke step sets `RUST_BACKTRACE=1`.
  - When no diagnostics arrive, the test's failure names the document's URI and every URI that
    has diagnostics.

**Round 1, the inventory.** The first push, asked, gives every failure on the legs. The run is
recorded here by class, and each class goes to the row that owns it, or to a new one. From the
code, these are expected:

- Windows:
  - vendor context empty, from the verbatim spelled paths (R3);
  - include-rule and URI tests (R3, R5, R8);
  - `/`-spelled report tests (R6);
  - `stub_index` (R14);
  - temp paths spelled with the 8.3 short name (`RUNNER~1`) against canonical ones (R13);
  - the smoke round trip (R3, R5);
  - the dhat build: the frame-pointer walk calls `pthread_getattr_np` (R14).
- macOS: temp paths under `/var/folders` against canonical `/private/var/…` ones (R13).

**Round 1's result.** Rust CI run 39124821751 on `d8de9530`. The logs are in
`~/.cache/prs-profile/fixes/logs/round1-*`.

- **aarch64 Linux:** every step passed.
- **gates (x86_64 Linux):**
  - The lockfiles, format, clippy, tests and fixtures passed.
  - The module graph failed with 27 problems, and the 13 steps after it were skipped.
  - The failure predates this plan. `crate::file_id` (`1cd01bc4`, 2026-10-02) came after the
    graph was last settled (`c7ffe38f`). Since then no run had reached the step, because R0's
    lockfile stopped every run before it.
  - The problems are the crate split's (WORK_STACK level 2): 20 edges the layering forbids,
    6 allow lines whose edge is gone, and phpdoc's dependency on types.
  - Whether this batch fixes them is asked with round 2's push. Until then, `54e7e02d` runs
    each gates check whatever the one before it gave.
- **macOS:** lib 47 of 1202 failed, composer 1 of 67, fixtures 1 of 3832.
  - 46 LSP tests: the server published, and navigated to, canonical URIs
    (`file:///private/var/…`), where the test client had sent `file:///var/…`. This goes to R5's
    extension.
  - `collect_php_files_recurses_into_subdirectories_and_skips_non_php` compares canonical results
    with the temp directory's spelling (R13).
  - `the_entries_are_sorted_by_name_bytes` writes `a.php` and `A.php`, which are one file on
    APFS (R13).
  - `psr4_locations_are_checked` (R13). The fixture tracks `src/Http/` and `src/http/`, and a
    case-insensitive checkout merges them into `src/Http/`. The walk resolves the configured
    `src/http/Router.php` to the disk's spelling, which is where PSR-4 maps the class. So no
    `psr4.caseMismatch` is reported, rightly for that disk.
- **Windows:** clippy failed on one unused import, and the steps after it ran anyway. lib 52 of
  1177 failed, cli 1 of 44, file_cache 1 of 5, stub_index 1 of 1, composer 9 of 60 and fixtures
  26 of 3832. The dhat build failed.
  - **Clippy.** `tests/package_ownership.rs` imports `resolve` for a `cfg(unix)` test (R13, with
    the links). Clippy stopped at that crate, so `54e7e02d` adds `--keep-going`.
  - **The verbatim prefix** (R3, built; checked on round 2):
    - Published URIs were `file://%5C%5C%3F%5CC:/…`, with the prefix encoded into the
      authority.
    - The vendor context was empty in 9 tests: `workspace_config` and `workspace_scan` (3), the
      cli's `check_project_auto_detects_composer_vendor_and_never_reports_on_it`, the file
      cache's `a_warm_run_reads_a_generated_autoload_files_entries_back`, and composer's vendor
      discovery and listing tests (4).
    - 9 include fixtures: `__DIR__` held `\\?\`, so `__DIR__ . '/lib/x.php'` named no file.
  - **The 8.3 short name.** The same 46 LSP tests as on macOS failed. The test client sends
    `file:///C:/Users/RUNNER~1/…`, while the canonical path is `C:\Users\runneradmin\…`. Like
    macOS's `/var`, this goes to R5's extension.
  - **Prefixes hashed and ordered by their text** (R4, built):
    - `sorted_by_key_is_sorted_by_path` failed on the mix's `//a/b` shares;
    - `an_entry_made_from_its_directory_is_the_path_made_whole` failed on a `\\a` directory,
      which pushing a name turns into a share.
  - **`path_bytes`.** Two mixes count no paths read by bytes (R16).
  - **URIs.**
    - `path_to_uri_preserves_a_literal_backslash_in_a_real_unix_filename` runs on Windows (R5).
    - `a_file_that_crashes_the_parser_does_not_take_down_the_lsp_session` sends `file://`
      followed by a Windows path (R13).
  - **Unix spellings in tests** (R13): `the_entries_are_sorted_by_name_bytes` fails as on macOS
    (NTFS), and so does `collect_php_files_…`.
  - **`stub_index`.** `index.tsv` is stale, because the paths are spelled with `\` (R14).
  - **The fixture harness's path column.** It prints the relative path with `\` (`src\Ext.php`).
    14 fixtures differ only there: pest 6, phpstan_api_gating 5 and composer_hygiene 3. This is
    R13's, and it comes with:
    - `restricted_symbol_usage`: `allowIn` `legacy/*` never matched `legacy\a.php` (R6);
    - two composer_hygiene fixtures, `provide_scripts_and_platform_are_checked` and
      `psr4_locations_are_checked`: the messages name the file as `src\Logger.php` and the
      PSR-4 target in its canonical spelling (R6). The second also misses its case mismatch, as
      on macOS.
  - **The dhat build.** It calls `pthread_getattr_np` (R14).
- **Extension CI** passed on all three systems, as the State records.

**Round 2's result.** Rust CI run 39138433432 on `52844b07`. The logs are in
`~/.cache/prs-profile/fixes/logs/round2-*`.

- **aarch64 Linux:** every step passed.
- **gates (x86_64 Linux):**
  - The module graph failed as in round 1 (R18).
  - Since `54e7e02d`, the steps after it ran for the first time on this branch. Four of them
    fail for causes older than the plan, and all four go to R18's step 8:
    - **Fuzz targets compile:** `fuzz/src/lib.rs:68` passes `project::parse_file` four
      arguments, but it takes three: the interner argument is gone.
    - **Each feature on its own:** `hotpath-alloc` alone does not build. It turns on
      `hotpath/hotpath-alloc` but not the package's own `hotpath`, and hotpath's alloc mode
      refuses to build without its profiler (`compile_error!`).
    - **Unused dependencies:** the package lists `memchr` and uses it nowhere.
    - **Licenses, advisories:** RUSTSEC-2026-0346, -0347, -0348 and -0349 are in rustls
      0.23.45, which mysql brings in. The fix is 0.23.46 or later.
- **macOS:** lib 3 of 1198 failed, composer 1 of 68, fixtures 1 of 3832. R5's client
  spelling fixed 44 of round 1's 46 LSP failures.
  - `did_change_workspace_folders_removed_clears_its_files_and_keeps_the_others`. The removed
    folder's file was cleared on its canonical URI (`file:///private/var/…`), and it had been
    published on the client's (`file:///var/…`). This is a product bug of R5's, so the editor
    keeps the stale diagnostics. It goes to R5's round-2 follow-up.
  - `a_superseded_run_sends_nothing_and_the_newer_run_sends_its_result` looks up
    `Server::last_diagnostics` by the temp directory's spelling, which the server keys by the
    canonical path (R13).
  - `collect_php_files_…`, `the_entries_are_sorted_by_name_bytes` and `psr4_locations_are_checked`
    fail as in round 1 (R13).
- **Windows:** lib 4 of 1173 failed, composer 3 of 75, stub_index 1 of 1, fixtures 23 of 3832.
  The dhat build failed, and so did clippy.
  - **Clippy.** Ten imports and two helpers are left unused on Windows by test files whose tests
    are all `cfg(unix)`. They are in `lsp/tests/symlink_identity.rs`,
    `project/tests/session_identity.rs`, `project/tests/walk_identity.rs`, `project/walk.rs:924`
    (`add_resolved_dir`, `add_resolved_file`) and composer's `tests/package_ownership.rs`. They
    go to R13 with the links.
  - **The verbatim prefix is gone** (R3): the vendor-context tests and three of the nine
    include fixtures pass.
  - **The same two LSP tests as on macOS fail**, for the same causes (R5, R13), and
    `a_file_that_crashes_the_parser_…` fails as in round 1 (R13).
  - Round 1's other classes fail again:
    - `collect_php_files_…` and `the_entries_are_sorted_by_name_bytes` (R13);
    - `path_bytes`' two mixes (R16);
    - `stub_index` (R14);
    - the dhat build, from `pthread_getattr_np` (R14).
  - **Fixtures:**
    - six include fixtures. The harness's `<fixture>` replacement misses the canonical
      spelling the messages use (`<fixture>/../../../..\fixtures\…`), which is R13's
      `portable`;
    - `allowIn` (R6);
    - the path column of pest 6, phpstan_api_gating 5 and composer_hygiene 5, with the
      composer_hygiene messages' own spelling (R6, R13).
  - The dhat build also warns of two imports it leaves unused (`registry.rs:78`,
    `project/declared_classes.rs:62`), on Linux as well (R14).

**Round 3's result.** Rust CI run 39181642548 on `c5bec58c`. The logs are in
`~/.cache/prs-profile/fixes/logs/round3-*`.

- **Passed:** x86_64 Linux's tests, tests without default features and fixtures; every
  platform's clippy and heap profiler build; aarch64 Linux's tests and fixtures; macOS's
  fixtures. The Windows leg ran R3 to R15's tests for the first time and passed all but three:
  - composer's 89 (the junction and file-link tests among them, R13);
  - the CLI's 44 (R1's deep-nesting test, R10's stack);
  - the file cache's 5 (R11's refresh);
  - `stub_index`'s 1 and `stub-sources`' 4 (R14);
  - `syntax`'s 300 (R13's line breaks);
  - xtask's 20 (R15) and the heap profiler's 3 (R14).
- **lint** failed three steps:
  - **Format.** The patched dhat's `build.rs` and `lib.rs` were not rustfmt-clean (R14).
    `cargo fmt --all` formats a path dependency's crate as well. The other session fixed it
    (`2c6c4502`).
  - **Docs.** `diagnostics`' crate doc linked `report_path`, which is a function and a module
    (R6). It links `report_path()` (`bdc49385`).
  - **Benches type-check.** `cargo check --release --workspace --benches` builds the lib's test
    target without debug assertions, where `registry::build_declarations`, which the unit tests
    call, was not re-exported (R14). It takes the `cfg` `any(debug_assertions, test)`
    (`ccb312b4`).
- **macOS:** one of 1,200 lib tests, `the_body_runs_on_a_thread_with_the_stack_size_asked_for`
  (R10). 67,120,768 bytes remained on the 64 MiB stack, 11,904 more than the size asked for.
  The upper bound has the megabyte the lower one has (`9e1f8ec9`). The 46 LSP tests of round 1
  pass.
- **Windows:**
  - **lib,** one of 1,336:
    `a_backslash_separates_names_in_the_windows_world_and_is_part_of_a_name_in_the_unix_world`
    (R8). Where the host is Windows, `Path` reads the Unix world's `\` as a separator, so the
    assertion that the Unix world does not find the file holds on Unix hosts alone: it is
    `cfg(unix)` (`e45a69c0`).
  - **fixtures,** 9 of 3,847, all `checker/includes` (R8). The Unix world took `__DIR__` for a
    relative name, so seven fixtures reported nothing, and the `dirname(__FILE__)` of it was `.`
    in two messages. R8's follow-up has the fix (`e45a69c0`).
  - **`path_bytes`,** the two mixes that count no paths read by bytes (R16), as in rounds 1 and
    2. R16 reads Windows paths (`9c555663`).

### R3. One canonical form of a path, without Windows' verbatim prefix

**Now.**

- Ten call sites call `std::fs::canonicalize`: `session.rs:326` (`identify_through`) and `:578`,
  `walk.rs:1231`, `manifest.rs:266` and `:344-348` (`resolve`), and `vendor.rs:120`, `:626`,
  `:639` and `:724`.
- On Windows each returns `\\?\C:\…`, so every root, listed file, discovered vendor path and
  document key carries the prefix. In a verbatim path `/` is not a separator and `.` and `..`
  are literal. As a result:
  - discovery's spelled paths (the canonical generated directory joined with
    `'/../symfony/console/X.php'`, `vendor.rs:381-394`) are one component that no listing holds,
    so a Windows load has no vendor context;
  - the include rule's `__DIR__` and `__FILE__` (`includes.rs:223-228`) hold `\\?\C:\…` where
    PHP holds `C:\…`, so `require __DIR__ . '/x.php'` is reported missing;
  - `path_to_uri` puts `\\?\` into a non-open file's URI (R5);
  - the reports show it.

**Change.**

- A module `composer::canonical` with `canonicalize(path) -> io::Result<PathBuf>`: the system's
  canonical path.
  - On Windows, `\\?\X:\…` becomes `X:\…` and `\\?\UNC\server\share\…` becomes
    `\\server\share\…`, when every component can be spelled without the prefix: no reserved
    device name (`CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9`, with or without an
    extension), no trailing `.` or space, and no character a Win32 name forbids. Otherwise the
    path stays verbatim.
  - There is no length limit: std itself adds the prefix to a long path when it calls the
    system.
  - Other verbatim forms (`\\?\Volume{…}\`) stay as they are.
- `manifest::resolve` builds on it, and all ten call sites use one of the two.
- `clippy.toml`: `disallowed-methods` for `std::fs::canonicalize` and
  `std::path::Path::canonicalize`. The one wrapper `#[expect]`s it.
- `HashedPath::child` (`hashed_path.rs:46`) keeps the invariant: a name that cannot be spelled
  plainly makes a verbatim child, so `Resolved::known`'s assertion (`:131`) holds on Windows.

**Test.**

- `cfg(windows)` unit tests of the rule:
  - a drive in either case;
  - UNC;
  - each reserved name, with and without an extension;
  - a trailing dot or space;
  - a long path;
  - a volume GUID path.
- A Windows round trip: a temp file's canonical path is plain, and `resolve` maps it to itself.

**Verify.**

- Linux is unchanged: the wrapper is `fs::canonicalize` there.
- On round 2:
  - a vendor discovery test finds vendor context on Windows;
  - the smoke test's round trip.

**Built.**

- `composer::canonical` holds `canonicalize` and one rule, `form`, for the spelling a
  canonical path takes. On Windows the rule works both ways: a verbatim drive or share path
  whose names are all spelled plainly loses the prefix, and a plain one with a name that is not
  takes it. The device names also cover `CONIN$`, `CONOUT$` and the superscript ports
  (`COM¹`), and a stem's trailing spaces (`CON .txt`). The rule is applied in three places:
  - the system's answer;
  - `resolve`'s tail through the nearest existing ancestor;
  - `HashedPath::child`, whose entry of a canonical directory is spelled as `form` spells it.
- The bans are in both `clippy.toml`s: `crates/phpstan-rs` has its own, and clippy reads only
  the nearest. They fire: a probe call of each failed `clippy -D warnings`. The wrapper
  `#[allow]`s the lint, as the workspace writes its exceptions.
- The arch gate failed at HEAD: `walk.rs:1231` has called `fs::canonicalize` since `aab67287`,
  in the engine its path rule covers. The rule exempts composer, which owns the path I/O (the
  walk reads its listings through it too), so the walk now asks `composer::canonical`, and the
  gate passes.
- The test code's 34 calls compute their expected paths with `canonical::canonicalize`, so they
  match the code's form on Windows. Two LSP test files lost their now unused `use std::fs`.
- The Windows code is type-checked here. composer cannot be checked for Windows, since
  stacker's build compiles `windows.c` with the MinGW headers, so
  `~/.cache/prs-profile/fixes/wincheck/check.sh` symlinks the pure-Rust path modules and their
  tests into a scratch crate. It runs `clippy -D warnings` for `x86_64-pc-windows-gnu` and for
  Linux. Nothing links, so nothing runs: the Windows leg runs the tests.
- Linux: the composer tests (68) and the `project::`, `lsp::tests::symlink_identity`,
  `lsp::tests::workspace_folders` and `checker::phpstan_api` tests (215) pass.

### R4. Equal paths hash alike on Windows

**Now.** `HashedPath::new` (`:33`), `path_hash` (`:149`) and `hashed_ancestors` (`:157`) fold
`component.as_os_str()`. A prefix's text is its spelling, while `Path`'s equality compares the
prefix's parsed kind (std uppercases the drive letter, and reads `//server/share` as
`\\server\share`). So `c:\a` and `C:\a` are equal `Path`s with different hashes, and a table
keyed by `PathHash` with path equality misses one of them. The module's doc (`:1-6`) promises
that they hash alike.

**Change.** One `component_hash(parent, &Component)`:

- a prefix hashes its kind: `Prefix::Disk(b'C')`, `UNC(server, share)` and the others, as
  `Path` compares them;
- any other component hashes its text, as now.

Unix has no prefix, so every hash there stays the same.

**Test.** `cfg(windows)`: the spellings `Path` calls equal (drive case, UNC separators) hash
alike. The existing `hashed_path` tests run on every leg.

**Built.**

- `component_hash` hashes a prefix by its kind and any other component by its text, as before.
  `new`, `path_hash` and both branches of `hashed_ancestors` fold it, and `child` extends a
  directory's hash as before. Unix's hashes are the same values.
- Two more places keyed a prefix by its text, and they are fixed alike:
  - `path_order`'s key. `\\server\share` and `\\server-b\share` ordered one way by key and the
    other way as `Path`s, and round 1's Windows leg failed `sorted_by_key_is_sorted_by_path` on
    the mix's `//a/b` shares. The key now writes the kind, as a byte in the order the kinds are
    declared, and what the kind holds, each name ending in a zero byte.
  - The package index's `DirTrie`: `c:\project\src` found no owner for a root listed as
    `C:\project`. It keys a component by `canonical::component_text`, which spells a prefix as
    std reads it.
- `cfg(windows)` tests:
  - spellings `Path` calls equal hash alike, through `path_hash`, `new` and every ancestor, and
    eleven prefixes it tells apart hash apart;
  - the key orders 18 prefixes, each with 7 tails, as `Path` does, pair by pair;
  - a directory has one owner in each spelling, and a verbatim drive is another prefix.
- The `hashed_path` mix is shaped per platform. On Windows it starts with each kind of prefix in
  each spelling and uses both separators. It has no empty name there: after a root, an empty
  name makes `\\a`, which pushing a name turns into the share `\\a\b`. That is why round 1
  failed `an_entry_made_from_its_directory_is_the_path_made_whole`, on a directory no canonical
  path can be. Unix's mix is the same sequence as before.
- Checked here:
  - the Windows type check, which now covers `path_order.rs`;
  - the ownership test, compiled on Linux under a cfg flipped for the check;
  - composer's clippy and its 68 tests on Linux.

  The Windows tests run on round 2.

### R5. URIs and paths on every platform

**Now** (`lsp/protocol.rs`).

- `uri_to_path` (`:110`):
  - It ignores the authority. On Windows `file://server/share/a.php` becomes `\share\a.php`; on
    Unix another host's URI is taken as local.
  - It strips the `/` before a drive letter on every platform. On Unix `file:///c:/x` is the
    absolute path `/c:/x`.
  - It decodes the escapes lossily (`percent_decode`, `:192`), so on Unix a name that is not
    UTF-8 (`%E9`) loses its bytes.
- `path_to_uri` (`:122`, `path_to_uri_path_string` `:158`):
  - It writes a prefix raw: `\\server\share` is percent-encoded into the path, and `\\?\C:`
    into the authority.
  - It writes a non-UTF-8 Unix name lossily.
- The doc comment at `:147-157` calls Linux "this project's only build target".

**Change.**

- Unix:
  - `file:///p` and `file://localhost/p` give the path made of the decoded bytes
    (`OsStrExt::from_bytes`), and another host gives `None`;
  - `path_to_uri` encodes the name's bytes.
- Windows:
  - `file:///C:/p` and `file:///c%3A/p` give `C:\p`, decoded as UTF-8 (anything else gives
    `None`);
  - `file://server/share/p` gives `\\server\share\p`;
  - `path_to_uri` writes `Disk` and `VerbatimDisk` as `/C:/…`, and `UNC` and `VerbatimUNC` as
    `file://server/share/…`. Other verbatim kinds keep their spelling, encoded.
- The comment states why components are joined with `/` on every platform.
- An open document keeps publishing on the URI its client sent (`open_uris`, `lsp.rs:1011`).

**Test.** Per-platform unit tests:

- `cfg(unix)`: bytes, `localhost`, another host, `/c:/`;
- `cfg(windows)`: both drive encodings, UNC both ways, and a verbatim path's URI.

Round 2 runs the smoke round trip. If it still fails, the smoke test prints the server's stderr
on failure, and the cause becomes a row.

**Round 1's extension: a file's URI in its client's spelling.**

- **Now.** A file the client has not opened is named by its canonical path's URI. That covers a
  published diagnostic and a definition, reference or rename edit in such a file; each one goes
  through `uri_and_version_for` (`lsp.rs:1010`). The client may spell the workspace another
  way:
  - macOS's `/var` is a link to `/private/var`;
  - Windows' `%TEMP%` holds the 8.3 name `RUNNER~1`;
  - on any system, a workspace can be opened through a link.

  An editor shows such a URI as another file. That is how the 46 LSP tests failed on macOS and
  Windows.
- **Change.** A file that is not open gets the URI of the deepest workspace folder whose
  canonical root holds it, with its components below that root appended. A file under no folder
  keeps its canonical URI, and an open document keeps the URI its client sent, as now.
- **Test.**
  - The 46 tests, unchanged, on every leg.
  - Here: a workspace opened through a link gets its diagnostics and definitions on the link's
    URIs.

  Linux's URIs change only for a workspace opened through a link, which they named wrongly.

**Built.**

- `uri_to_path` reads a `file:` URI by RFC 8089. It refuses another scheme, a user, a port and
  a NUL byte. It decodes strictly with its own decoder, which replaces the lossy
  `percent_decode`.
  - Unix: the host is empty or `localhost`, in any case, and the path must be absolute. The
    name is its bytes. `file:///c:/x` is `/c:/x`.
  - Windows: an empty host needs a drive. `/C:/p` and `/c%3A/p` give the drive's path; the
    letter keeps its case, which `Path` equality ignores. A drive alone is the drive's root,
    and a drive-relative path names nothing. Any other host is a share and needs one:
    `\\server\share\p`, `wsl%24` decodes to `\\wsl$\…`, and the hosts `?` and `.` give the
    verbatim and device namespaces. The text must be UTF-8.
- `path_to_uri` writes each name after a `/`, percent-encoded with the same kept set as before
  (the unreserved bytes and `:`).
  - Unix: the name's bytes, so `\` and bytes that are not UTF-8 survive.
  - Windows: a drive or verbatim drive is `/C:`, with the letter std reads. A share or
    verbatim share is its host and share name. The verbatim and device namespaces are the
    hosts `%3F` and `.`, as VS Code spells them.
- The client's spelling: `uri_and_version_for` names a file that is not open by
  `client_spelling`. That is the path under the deepest folder whose resolved root holds it,
  spelled under the root as the client named it (`Folder::spelled`). The folder's URI is made
  from that spelling, and clients decode a URI before comparing it.
- Tests:
  - per platform, for both directions: the refusals, `localhost`, another host, `/c:/`, bytes,
    `\` in a Unix name, both drive encodings, shares both ways, `wsl$`, and the two
    namespaces;
  - a workspace opened through a link has its unopened file's diagnostics published, and a
    definition answered, under the link's URIs;
  - the existing link test now expects the target under the folder's spelling. It held on
    Linux only because the temp directory there is canonical.
- Checked here:
  - clippy on Linux, and the 287 `lsp::` tests;
  - a mutation that turns `client_spelling` off fails the new link test;
  - the Windows type check, which now also compiles the URI functions and their tests with
    lsp-types.

  Round 2 runs the Windows and macOS legs and the smoke round trip.

**Round 2's follow-up: a removed folder's files are cleared on the URIs they were published
on.**

- **Now.** When a workspace folder is removed, the server clears its files' diagnostics after
  the folder has gone. `client_spelling` then finds no folder for them, so the clear goes out
  on the canonical URI. The client never published there, and it keeps the stale diagnostics
  (round 2, macOS and Windows). A workspace opened through a link has the same bug on Linux.
- **Change.** A clear goes out on the URI the file's last publish used. The server keeps, for
  each file it has published, the URI it published on, and clears that one. It does not
  recompute a spelling once the folder that gave it has gone.
- **Test.** The round-2 test on every leg. Here: a workspace opened through a link and then
  removed clears its files on the link's URIs.

**Built** (`8d81da46`), after R18 and before R6.

- `Server::published` replaces `last_diagnostics`. For each file it holds the diagnostics the
  editor holds and the URI they went to, `None` when none were sent.
- Every publish goes through one function, `publish`.
  - A set goes to the URI the editor knows the file by now. When the editor holds the file's
    diagnostics under another URI, they are cleared there first.
  - An empty set is that clear alone.
- `publish_analysis` also sends a file whose diagnostics are unchanged when the editor holds
  them under a URI it no longer knows the file by, as when a folder spelled otherwise comes to
  own it.
- The disabled branch's clears go through `publish` too.
- Tests:
  - a workspace opened through a link and then removed: its file is published, then cleared,
    on the link's URI alone;
  - a file published under the link and then opened by its real spelling: the link's URI is
    cleared, and the diagnostics go to the URI it was opened by.
- Checked here:
  - both tests fail with the move turned off, and pass with it;
  - the 289 `lsp::` tests and clippy pass.
- Linux's output changes only where a file's URI changes: a workspace opened through a link, or
  a file opened, or reported by the file watcher, under another spelling.

**Found while building it, older than this plan and on every platform.**

- While `phpstan-rs.enable` is false, an edit still publishes the file's diagnostics:
  `handle_open_or_change`, the debounced change and the watched files never read `enabled`.
- Disabling clears only the open files. A file the workspace scan published and the editor
  never opened keeps its diagnostics, though the extension's setting says that disabling
  "clears its diagnostics". `published` now records every file the editor holds diagnostics
  for, so the clear could cover them all.

**Round 2's second follow-up: a disabled server does nothing visible** (decision 6), built
after R6's corrections.

- **Now.**
  - The setting says "Disabling stops the server and clears its diagnostics", and the
    extension stops its client when `enable` is false at start. The server's own comment says
    the opposite of the setting: navigation keeps working and only the diagnostics go empty.
  - `enabled` is read in two places, `analyse_on_save` and `republish_known_files`. A
    didOpen, a didChange (also through the debounce worker), a watched-file event, a folder
    added or removed and a configuration change that moves a file's role all analyse and
    publish while it is false.
  - Disabling clears the open documents only.
  - The requests answer from the project, and `inlayHint` runs the checker.
  - The extension asks for a restart after `enable`, `serverPath` and `configPath` change.
    The server applies `enable` and `configPath` live (`reload_configuration`).
- **Change.**
  - `publish` is the one place a diagnostic leaves the server, so while disabled it sends
    only the clear of what the editor holds. That makes publishing impossible by
    construction, whatever triggered it.
  - The entry points that analyse do no work while disabled: an edit records the document
    (`open_uris`, `latest_text`, `file_versions`) and returns, and so do the debounced
    change, the watched-file events, a folder added or removed, and a configuration change
    that is not `enable` itself.
  - Disabling clears every file in `published`. Enabling brings the open documents to the
    projects (`sync_open_documents`), reloads what the disk changed meanwhile, runs the full
    analysis and publishes.
  - Every request answers its empty result while disabled, and the doc comment on `enabled`
    says so.
  - The extension stops the client when `enable` turns false and starts it when it turns
    true, with no prompt. `configPath` prompts nothing, since the server applies it.
    `serverPath` restarts the client, since it names the process.
- **Test.** The server tests: an edit, a save, a watched-file change, a folder change and a
  configuration reload while disabled publish nothing; disabling clears a file nothing
  opened; each request answers empty; enabling publishes what the editor holds, including an
  edit made while disabled. The extension's unit test of the setting handlers.

**Built** (`7a757a8e`).

- `publish` sends only the clear of what the editor holds while disabled, and nothing for a
  file it holds nothing for. Every trigger goes through it.
- No work while disabled:
  - `analyse_and_publish` is the save's, the load's and the republish step's one analysis, and
    a disabled server runs none;
  - `handle_open_or_change` and `handle_debounced_change` keep the document (`open_uris`,
    `latest_text`, `file_versions`) and return;
  - `handle_watched_files_changed` keeps the events (`deferred_file_events`), and
    `sync_open_documents` does nothing.
- Disabling clears every file in `published`, in path order. The old branch cleared the open
  documents.
- Enabling runs `catch_up`: the open documents go to the projects, the deferred events are
  handled, and the full analysis publishes the result.
- `FEATURE_REQUESTS`, the 13 requests that ask what the server knows of the code, answer
  `null`. The existing test that asserted hover works while disabled now asserts the opposite,
  and that it works again after enabling.
- The extension:
  - `changeNeedsNewProcess` (`config.ts`, no `vscode` import) says that `enable` and
    `serverPath` need a new process and `configPath` does not;
  - every start and stop goes through `serialized()`, one after the other;
  - the prompt is gone. The README and the stale comments say what is true.
- Tests:
  - five server tests (`lsp/tests/disabled_server.rs`): a file nothing opened is cleared; an
    open, an edit, a save and a watched file send nothing, change nothing in the project, and
    are applied on enabling; a folder added while disabled is analysed on enabling; an edit
    over the wire publishes nothing; all 13 requests answer `null`;
  - the helpers three test files each defined (`as_values`, `publishes_for`, `diagnostics_of`,
    `loaded_server`) moved to `helpers.rs`;
  - the extension: four cases of `changeNeedsNewProcess` and three of `serialized`.
- Checked here: four mutations (the debounced gate, the request gate, the clear of open files
  only, the catch-up) each fail a test; clippy; the 294 `lsp::` tests; the extension's 41 unit
  tests. The smoke test runs on the CI legs.

### R18. The gates job: every module has its crate, no edge breaks the layering, and the later steps pass

Built after R5, in the place the table gives it. The user's decision with round 2's push: the
forbidden edges are refactored away, not listed as exceptions. This is crate-split work
(`WORK_STACK.md` level 2) pulled forward.

**Now.** Round 1's gates job failed `cargo xtask modgraph` with 27 problems, read at `52844b07`:

- **14 edges into the crate root.** `crate::file_id` (`1cd01bc4`, 2026-10-02) has no row. So the
  `crate` row holds it, and no crate may depend on the root. 12 modules of six crates name it,
  and the registry and the project name `crate::shard` too, which has no row either. The 14th
  is `main`'s call of `phpstan_rs::keep_freed_memory`, a function of the root.
- **6 edges that break the layering.**
  - `registry::init_flow` (reflection) uses the checker's completion rules (`Completion`,
    `Reach`, analyser) and `is_truthy_literal` (`checker::control_flow::boolean_conditions`,
    rules).
  - `project.rs` reads two rules' mention bits and spellings: `checker::unused::MENTIONS_REBIND`
    and `checker::includes::MENTIONS_GUARD` (`:2528`, `:2531`), and their `WATCH` lists in
    `WATCHED` (`:3064`). A project test reads them too. These are two edges.
  - `checker::access_attributes` (ext-language-extensions) calls `project::shown_path`
    (`7bef998a`).
  - `pest_config` (ext-pest) calls `composer::path_bytes` (`30aa2002`). No extension may depend
    on composer.
- **6 allow lines whose edge is gone:** `checker::control_flow project`,
  `checker::uninitialized_dataflow project`, `lsp banned`, `lsp ext`,
  `lsp ext::disallowed_calls` and `lsp ext::sql`.
- **phpdoc's manifest depends on types** (`2b6aeb84`), which phpdoc's row does not list.

The gate let a new module fall through to the `crate` row without a word. A module the root holds
has its own edges unchecked, since the root may depend on everything. `gate` and `allocator` have
no row either.

**Change.** One commit for each step.

1. **The gate.** A module that only the `crate` row holds has no assignment. That row is the
   library root's own, which only declares modules and re-exports.
2. **Rows for the four modules.**
   - `file_id` and `shard` go to reflection. The registry is their lowest user, and they need
     nothing but std, rayon and the hash crates.
   - `gate` goes to testing, Appendix A's fixture runner. It reads a fixture's config against
     the schema its caller passes, as the language server does. The fixture harness and the
     runtime oracle pass `app::config_schema()`.
   - `allocator` goes to bin, since the binary chooses the allocator.
     - It becomes the one module for it, always built. It holds the `#[global_allocator]`
       static and `keep_freed_memory`, which does nothing under another allocator.
     - The mimalloc binding moves to `allocator::mimalloc`.
     - `main` calls `phpstan_rs::allocator::keep_freed_memory()`.

     The root then only declares modules and re-exports.
3. **The completion rules go to syntax.**
   - `Completion`, its rules, `Reach` and `BlockCompletion` are pure functions over the
     completions of a statement's parts and what is known of its conditions. The 25 G report's
     C1 step 6 has them shared by the checker and `InitFlow`.
   - They move to `syntax::completion`, which both use.
   - `is_truthy_literal`, the syntactic answer to "always true", moves to `syntax::ast`, beside
     `literal_string_value`.
   - The exit points and the goto state hold the checker's scopes, so they stay in the checker.
4. **The checker's walks own their mention bits in the analyser.**
   - The bits and spellings that let `check_program` skip the rebind scan and the include-guard
     walk move to `checker::mentions`. That is the analyser module whose code reads them.
   - The project hands the checker the file's whole `mentions` (`set_mentions`, which replaces
     the two flag setters).
   - The project composes `WATCHED` from three layers' lists: `stub_attributes` (reflection),
     `checker::mentions` (analyser) and `file_declarations` (project).
   - A test holds the two walks' spellings and their scans together: every function name the
     include-guard walk reads, and every rebind call, is watched.
5. **The project gives the checker the file's shown path.** `set_current_file` takes the path
   as the reports spell it (`project::shown_path`), as the checker already takes `__FILE__`'s.
   The access attributes' `in_test_code` and the restricted symbols' path rules read it. The
   first called into project, and the second kept its own copy of the rule.
6. **`path_bytes` becomes a crate of its own,** `phpstan-rs-path-bytes` (lib `path_bytes`), at
   the bottom of the graph, with std as its only dependency.
   - It reads paths, not Composer's metadata. The analyser's and the extensions' layers may not
     depend on composer.
   - composer and the package use it. Its tests move with it, and its normal-form test takes
     std's components as its oracle: those are what `lexically_normalized` rebuilds.
   - xtask's arch and build-time lists name it. The Windows type check lints the crate itself.
   - R16 and its bench work in this crate.
7. **phpdoc may depend on types.**
   - The 25 G report decided this for P7 (§10.1, §11.7): types depends on syntax alone, so
     there is no cycle.
   - The `Type` a docblock writes lives beside its node, lowered once per distinct docblock
     (`2b6aeb84`). Resolution against names, templates and aliases stays in reflection.
   - The phpdoc row and `CRATE_SPLIT_PLAN.md`'s §3.3 now say so. The six stale allow lines go
     in the same commit, and the gate passes.
8. **The gates job's later steps,** which ran for the first time in round 2 and fail for
   causes older than this plan:
   - the fuzz library calls `project::parse_file` with its current arguments;
   - `hotpath-alloc` turns on the package's own `hotpath`, which its alloc mode needs;
   - the unused `memchr` dependency goes;
   - rustls goes to a release past the four advisories, by `cargo update -p rustls`, in both
     lockfiles where it appears.

**Test.**

- xtask: the gate's spec parsing is unit-tested, including a module that only the `crate` row
  holds.
- `cargo xtask modgraph`, `arch` and `tests` (the test-name manifest, with each moved test in a
  `gates/test-renames` file) pass.
- Each step's moved code runs its own tests, listed when it is built.

Linux output is unchanged: each step moves code and keeps what it does.

**Built.** One commit for each step.

1. `0e908059`: a module that only the `crate` row holds is reported, the topmost one only,
   and the spec's parsing is unit-tested on a two-crate spec.
2. `3773f312`: `file_id`, `shard`, `gate` and `allocator` have rows.
   - `fixture_config` takes the schema its caller passes.
   - The fixture harness and the runtime oracle build `app::config_schema()` once each.
   - The gate runner passes it too.
   - `allocator` is always built, with the binding in `allocator::mimalloc`, and the root
     holds no function.
3. `ac0010c9`: `syntax::completion` holds `Completion`, its rules, `Reach` and
   `BlockCompletion`, and `syntax::ast` holds `is_truthy_literal`. The checker keeps
   `ExitKind`, `ExitPoint` and `GotoState`.
4. `392e75b3`: `checker::mentions` holds the two bits and the watch list.
   - The project hands the checker the file's mentions.
   - `WATCHED` is composed from the three layers' lists.
   - Two tests hold the include-guard walk's function names and a rebind's spelling to the
     list.
5. `cbd9af81`: `set_current_file` takes the shown path, and the two rules read it.
6. `27091848`: `phpstan-rs-path-bytes`.
   - Its tests and the composer tests pass, 6 and 62, the 6 having been composer's.
   - So do the package's `project::` and `pest` tests, 192 of them.
   - The Windows type check lints the crate for both targets.
7. `c1a7e224`: the phpdoc row, `CRATE_SPLIT_PLAN.md`'s diagram and §3.3, and the diagram's
   `path-bytes`. The six allow lines go. The gate passes: 403 modules, 12,700 references and
   236 listed exceptions.
8. `80b3bbd9`: the later steps.
   - The fuzz library's call.
   - `hotpath-alloc = ["hotpath", "hotpath/hotpath-alloc"]`.
   - The package without `memchr`: no source, test or bench of it names the crate.
   - rustls 0.23.46 in both lockfiles. `cargo update -p rustls` unlocks every package below
     rustls, getrandom 0.4.3 among them (through aws-lc-sys's build dependencies), so it
     also moved tempfile's `>=0.3, <0.5` edge to 0.3.4. That edge is set back. Each lockfile
     changes only in rustls's version and checksum and the dropped `memchr`, and both pass
     `--locked`.

Checked here:

- clippy on the workspace, `-D warnings`;
- the lib suite (1200), the fixtures (3832), syntax (279) and the xtask tests;
- `cargo xtask modgraph` and `arch`;
- the gates job's later steps:
  - the fuzz targets' check;
  - `cargo hack check --workspace --each-feature --exclude-all-features`, 19 runs;
  - `cargo machete`;
  - `cargo deny check`, which reports `advisories ok`, with the duplicate-version warnings
    as before.

The test-name manifest compares against the one round 2's gates job saved, `52844b07`. Since
that commit seven tests have moved, the allocator's one and path_bytes' six, and each has its
line in `gates/test-renames/module-graph.txt`. No other module or test function has gone or
been renamed. Round 3 runs the comparison.

**Left as it is.** `cargo hack` gives one manifest warning under `dhat-heap`,
`hotpath-alloc` and `system-allocator`: `libmimalloc-sys` is unused there.
- The binding is built under every feature set, and used only when mimalloc is the
  allocator.
- Round 2 gave the same warning under `dhat-heap`.
- A feature cannot remove a dependency, so dropping it there needs a `mimalloc` feature
  among the defaults. That would change what `--no-default-features` builds.
- It is a warning and not a failure, so it is not changed here.

### R6. Machine reports spell a file with `/`, and a `\` in configured path text names a separator

**Now.** `shown_path` (`project.rs:3430`) gives `src\Foo.php` on Windows. So:

- `ignore_errors::filter` (`:134`) matches `to_string_lossy()` against `src/*`, which never
  matches;
- the baseline writes and matches `normalize_path(..).display()` (`baseline.rs:201`, `:393`).
  A baseline made on Linux does not match on Windows, and the other way round;
- checkstyle and junit (`output.rs:212`, `:244`) write `\`, so one project's report differs by
  platform.

**Change.**

- One function, `report_path(&Path) -> Cow<str>`. On Windows it gives the components joined with
  `/`. On Unix it gives the path's own text, since `\` is a name character there.
- The baseline writes it. On Windows it reads an entry's `\` as `/`, which covers baselines this
  tool wrote there before, and matches it.
- `ignoreErrors`: a pattern's `\` is a separator on Windows, where it is one in a path. The path
  and the pattern are compared in `/` form.
- Checkstyle and junit use `report_path`. Text, friendly and json kept the platform's spelling
  here; decision 5 moves json to `/` (Corrections below).

**Test.**

- Unit tests per platform.
- A baseline made with one spelling matches the other on Windows.

Linux output is unchanged.

**Round 1.**

- The restricted symbols' `allowIn` and `allowExceptIn` (`restricted_symbols.rs:484`) match the
  root-relative path with `ignoreErrors`' matcher, so on Windows `legacy/*` never matched
  `legacy\a.php`. They take the same `/` form.
- composer_hygiene's messages name the file as `shown_path` spells it (`src\Logger.php`), and
  the PSR-4 target in its canonical spelling. A baseline matches by message. So R6 settles the
  messages' spelling with the report's: a path below the root is spelled alike on every
  platform, and Linux's messages stay as they are.

**Built** (`2526323b`, `3350adbd`).

- `diagnostics::report_path` spells a path for reports and messages. On Windows each `\`
  becomes `/`. Elsewhere it is the path's own text, since a name can hold a `\` there. It is in
  the diagnostics crate because the app, the project, the rules and the disallowed-calls
  extension all call it.
  - It replaces text instead of joining components as the plan said. The two agree on every path
    a report holds, which has no `.` and no doubled separator. A share keeps its
    `//server/share` form, so a prefix needs no rule of its own.
- Where it is used:
  - checkstyle's and JUnit's file names;
  - the baseline's files, written and matched;
  - anonymous class names (`class@anonymous/src/a.php:2`), which appear in messages and so in
    baselines;
  - the restricted symbols' path for their zones;
  - the messages that name a file: the duplicate class and function declarations' location
    lists, and composer hygiene's two PSR-4 messages. A baseline matches by message, so a path
    in a message is spelled as a baseline's file is, and a baseline made on one platform
    matches on another.
- Text, friendly and JSON kept the platform's spelling, and so do the warnings on stderr.
  JSON follows the correction below.
- One rule matches configured path text against a report path (`3350adbd`).
  `diagnostics::path_byte_names` reads it byte by byte:
  - a `\` names a `/` on every platform;
  - a `\` also names a `\` that a name holds, which a report path has only outside Windows;
  - a `/` names only itself, and so does every other byte.

  It is applied in two places:
  - `ignoreErrors` paths and the restricted symbols' `allowIn` and `allowExceptIn` match
    through `ignore_errors::path_glob_matches`. `glob_matches` became one algorithm over
    bytes with the rule passed in, so a name pattern keeps `\` as PHP's namespace separator.
  - The baseline keys its entries by their slash spelling. In that spelling an entry's file
    equals every file it names, and `diagnostics::path_text_names` then confirms each
    candidate. An entry written with `\`, by hand or by this tool on Windows before this row,
    matches. Nothing is rewritten on load.
- composer hygiene names the PSR-4 target relative to the root, as it names the file. It used
  to give the canonical absolute path, on Linux too, which put the machine's directory into the
  message and so into every baseline entry for it.
- Linux's behaviour changes in two places, and both were defects there too.
  - The PSR-4 target's path, which changes two fixtures' expectations (3 lines). The rule is
    opt-in, and Piwigo's configuration does not enable it.
  - A `\` in configured path text now also names a `/`. Piwigo's configuration has no `\`,
    and neither do the 25 fixture configurations that set these paths.
- Tests:
  - `report_path`, and the rule's two functions;
  - on every platform:
    - a `\` in an `ignoreErrors` path, in a zone and in a baseline entry is a separator;
    - the rule holds at a glob's start, in its middle and at its end, and in a non-ASCII name;
    - a name pattern keeps its `\`;
  - on Windows:
    - `ignoreErrors` in either separator;
    - a Windows file written to a baseline with `/` and matched again;
    - checkstyle and JUnit name the file with `/`, while JSON kept `\` (corrected below);
    - an anonymous class's name;
  - outside Windows: a `\` in a pattern or a baseline entry also names a `\` in a name, and a
    `/` does not.
- Checked here, after each commit:
  - clippy, `-D warnings`, on the package and the diagnostics crate;
  - the diagnostics tests (13) and the touched modules' tests: 93 for the first commit, then
    47 in `baseline`, `ignore_errors` and `restricted_symbols`;
  - the fixtures (3832), with the two composer_hygiene fixtures re-blessed;
  - the CLI's baseline and ignore tests (2);
  - the Windows type check, which now covers `report_path` and its tests;
  - the package's Windows tests, compiled on Linux under a flipped `cfg` in one clippy run.

  `cargo xtask modgraph` (403 modules, 12,717 references) and `arch` pass.

**Found while building it.**

- **PHPStan reads a `\` in configured path text as a separator on every platform**, not only on
  Windows as this row said.
  - `FileHelper::normalizePath` turns each `\` into `/` for the pattern and the file alike,
    before `FileExcluder` compares them. `FNM_NOESCAPE` matters only on Windows, where the
    joined spelling has `\` again.
  - disallowed-calls' `FilePath::fnMatch` normalizes both sides the same way.

  So the plan's reading outside Windows, a `\` as a character of a name alone, would have
  left a configuration or baseline written on Windows unmatched on Linux and macOS. The second
  commit builds the rule above. "Not in this plan" was wrong about `\` escapes for the same
  reason: PHPStan's matcher never sees a `\`.
- **A configured root has the same question.** PHPStan reads `paths` through `normalizePath`
  too (`CommandHelper.php:251`). This tool reads a `\` in `paths`, `scan_paths`, `stub_paths`,
  `schema_file` and `baseline` as a character of a name outside Windows. Choosing between the
  readings needs the disk, and R7 probes each configured root against it, so R7 takes this.
- **Case:**
  - disallowed-calls matches its zones case-sensitively on every platform (`FNM_NOESCAPE`
    alone);
  - PHPStan's `FileExcluder` folds case on Windows only;
  - R7 folds where a root's directory does. That differs from disallowed-calls on Windows, and
    from both on macOS.
- **Older than this plan, on every platform:** the SQLite reflection's `path` is opened as
  written. A relative one is therefore relative to the process's working directory, while
  every other path in the config file is relative to the file. Decision 7 settles it, and R7
  builds it.

**Corrections** (decision 5), built before R7.

- **JSON names a file with `/`.** `write_json` keys `files` by `report_path`, in the order of
  that spelling, where it used the platform's. One project then gives the same JSON on every
  platform, as it gives the same checkstyle, JUnit and baseline. Text, friendly and the
  warnings on stderr keep the platform's spelling.
- **The reasons are the run-time ones.** The module doc of `report_path`, the docs of the
  `ignoreErrors` `path` and of `allow_in`, the baseline's module doc, the anonymous class
  names' doc and the checkstyle doc gave how PHPStan spells or reads a path as the reason.
  The reason is that a report, a baseline and a configuration are shared between platforms:
  a file needs one spelling in them, and configured text has to name the same files wherever
  it is read.
- **Test.** The Windows output test expects JSON in `/`, and a test on every platform holds
  JSON, checkstyle and JUnit to one spelling of a path.

**Built** (`daca16d5`).

- `write_json` keys `files` by `report_path` and sorts by that spelling.
- The docs name the reason: `report_path`'s module doc, the `ignoreErrors` `path` and `allow_in`
  docs, the baseline's module doc and the anonymous class names' doc. The checkstyle doc points
  at the other machine formats.
- Tests: JSON, checkstyle and JUnit name one path alike on every platform. A path that mixes
  `/` and `\` is spelled with `/` on Windows and kept as it is on Unix.
- Checked: clippy, `-D warnings`, and the 74 tests of the touched modules.

### R7. Case is matched as the root's directory matches it; configured paths

**Now.** Matching against configured text is exact on every platform. On NTFS, default APFS
or a Linux casefold directory, `Src/*` and `src/*` name the same files.

**Change.**

- **The probe.** Each configured root is probed once at load: the project root, `paths`, and the
  scan and stub roots. The probe takes an entry of the root's listing (from `DirListings`,
  which the walk then reuses) whose name has a letter and whose case-flipped spelling the
  listing does not hold. If `symlink_metadata` finds the flipped spelling, the root's directory
  folds case; if not, or when no entry qualifies, matching is exact. A vendor root takes the
  project root's rule.
- **What the rule applies to.**
  - The project root's rule serves `ignoreErrors` paths and the include rule's guard paths
    (`include_guarded_paths`).
  - Each walk root's rule serves its excluded names: each root's `exclude`, and the name floor
    of `.git`, `node_modules` and the vendor directory (`path_under_excluded_dir`,
    `project.rs:3419`).
- **Not the baseline.** Its entries come from the files' own spelling, so they stay exact.
- The project carries the rule.

**Test.**

- Matchers with the rule injected.
- The probe gives exact here, and folds on the Windows and macOS legs.

On Linux the probe gives exact, so the output is unchanged. Its cost is about one `lstat` per
configured root.

**Round 1.**

- The restricted symbols' `allowIn` and `allowExceptIn` are configured paths too, and take the
  project root's rule.
- macOS and Windows resolve a configured path to the disk's spelling, which is how round 1's
  PSR-4 fixture lost its case mismatch there. glibc's `realpath` builds its result from the
  names it is given, on a casefold directory too, so on Linux `psr4.caseMismatch` can report a
  file whose name on disk is right. On a root that folds case, the walk spells a configured
  path as its directories' listings do.

**Configured paths** (from R6; decisions 5 and 7).

- **One function resolves a configured path.** `Session::resolve` resolves `paths`,
  `scan_paths`, `stub_paths`, `schema_file` and `baseline` against the config file's
  directory through `resolve_against_config`. The SQLite reflection's `path` is the one
  configured path left out, and it takes the function too.
- **A `\` in a configured root.** A `\` there is a separator on every platform (decision 5),
  and outside Windows it can also be a character of a name. The probe reads each root against
  its directory's listing both ways:
  - a list's root (`paths`, `scan_paths`, `stub_paths`) gives each reading the listings hold,
    the separator reading first;
  - a single file (`schema_file`, `baseline`, the SQLite `path`) takes the reading the
    listings hold. When both readings name an existing file the configuration is ambiguous:
    it is an error naming both files, and the user spells the one meant;
  - when the listings hold neither, it is the separator reading, and a missing file is named
    in that reading.

  Windows already reads `\` as a separator.
- **The SQLite reflection opens read-only.** `SqliteReflector::connect` opens the file with
  `SQLITE_OPEN_READ_ONLY` and without the create and URI flags that `r2d2_sqlite`'s `file()`
  sets by default. The reflector only prepares statements and reads their columns, which a
  read-only connection does (checked: it prepares even an `INSERT`). A path that names no
  file is an error naming the resolved path. Today it creates an empty database there.
- **Prior art for the case rule:** disallowed-calls matches its zones case-sensitively on every
  platform, and PHPStan's `FileExcluder` folds case on Windows only.

**Built, the configured paths** (`1146b4f6`).

- `project::configured_path` reads a configured path's text.
  - Outside Windows, text with a `\` has two readings, the separator reading first and the
    name reading after. Text without one has one reading, and the disk is not asked about it.
  - `roots` gives a list entry's roots: each reading that exists, else the separator reading.
  - `single` gives a single file: the reading that exists, else the separator reading; when
    both exist it is an error naming both, and saying to spell the separator `/`.
- `Session::resolve` resolves `paths`, `scan_paths`, `stub_paths`, `schema_file`, `baseline`
  and the SQLite `path` through it and `resolve_against_config`. An editor (the degrade
  policy) drops an ambiguous baseline or schema, or an SQLite path that names no file, with a
  warning, and goes on without it.
- The SQLite reflection:
  - `Session::resolve` requires the resolved path to be a file. Otherwise the configuration is
    in error: `sql_reflection.path <resolved path> names no file`. A schema-only reflection
    has no `path` and is as it was.
  - `SqliteReflector::connect` opens read-only, with the default's `NO_MUTEX` and neither the
    create nor the URI flag, and its pool error names the path. The reflector's tests make
    their database through a connection of their own, and its session-init test writes a
    temporary table, which a read-only connection can.
- Tests:
  - the readings and both functions with the disk injected, and the text-with-one-reading
    case, which fails if `exists` is called;
  - `Session::resolve` for each setting: a path relative to the config file and not to the
    root (the config file in `proj/conf`, a decoy at `proj/db`), a path that names no file,
    the editor's warning, a baseline read as a separator, as a name and as both, and the same
    for `paths`.
- Checked here: three mutations (the SQLite path not resolved against the config file, the
  second reading ignored, the read-only flags replaced by the default) each fail tests;
  clippy; the project, app and sql tests; the CLI's baseline tests; the module graph; the
  Windows type check, which now covers `configured_path` and its tests.
- The test-name manifest runs only on a clean tree, so round 3 compares it. No test was
  removed or renamed: the SQLite reflector tests that were reworked kept their names.

**Built, the case rule** (`1fe8c8e4`).

- `diagnostics::CaseRule`, `Exact` or `Folded`.
  - `key` gives the text a rule compares: as it is, or with each letter in its simple lower
    case, taken through its simple upper case so that `ς` and `σ` share a key. A letter whose
    mapping is more than one letter (`ß`) and `İ` are left as they are.
  - A key of text that needs no folding is borrowed, so `Exact` costs nothing.
- The probe is `Listing::folds_case` (`DirListings::folds_case` for a directory): the first
  entry of a complete listing that has an ASCII letter and whose case-flipped spelling the
  listing does not hold, found or not by one `symlink_metadata`. An incomplete listing, or no
  such entry, is exact.
- `Session::resolve` probes the project root once. The listing it reads is the one the walk
  reads when the root is walked.
- **One rule serves everything, where the plan had one per configured root.** `exclude` is not
  a configurable key. The names a walk leaves out are the hardcoded floor, and the floor
  applies to the root itself (the editor's fallback when no `paths` are given) and to a file
  no root lists. Both lie under the project root. Every other pattern, an `ignoreErrors`
  path or a zone, is matched against the path relative to that root. The project root's rule
  is therefore the one that applies, and probing each root of `paths` would only repeat it.
- It serves:
  - `ignoreErrors` paths: `ignore_errors::filter` takes the rule through `Finalize`, and keys
    each file's path once;
  - the restricted symbols' `allow_in` and `allow_except_in`, through
    `CheckSettings::path_case` (`ProjectSettings::path_case`);
  - the include rule's guarded paths, keyed by the rule on insert and on lookup;
  - the names a walk leaves out: `walk::Excluded { names, case }` replaces the name slice
    throughout the walk. `Walk::with_path_case` gives it to the roots a caller adds, and
    callers' plain slices are unchanged. `Root::contains`, `path_under_excluded_dir`,
    `unlisted_role` and `Project::update_file_cancellable` use it too.
- The baseline stays exact.
- `project::path_spelling::as_listed` spells a configured root as the listings of its
  directories spell it, on a root that folds case. This is the plan's walk-side respelling,
  built in the session, where the rule and the listings are: `dirs`, `scan_dirs` and
  `stub_dirs` go through it.
  - Each plain name of a path below the root becomes the one entry the rule folds to it.
  - A name the listing does not hold or holds twice, a link, a file, and a component that is
    not a name end the respelling, and the rest of the path is as given.
  - Why: glibc's `realpath` keeps the spelling it is given on a Unix casefold directory, where
    macOS and Windows give the disk's. Without it a root written in another case is a second
    key for the files of the first.
  - `schema_file`, `baseline` and the SQLite `path` are opened, not keyed, so they are not
    respelled.
- Tests:
  - `key`: equal keys for the spellings of a name (ASCII, `É`/`é`, `ΟΔΟΣ`/`οδος`, the Kelvin
    sign), distinct keys for what simple mappings keep apart (`straße`/`strasse`, `İ`/`i`),
    and a borrowed key for folded text;
  - the probe, with hand-made listings over real directories so that a case sensitive disk
    shows what a folding one gives: a flipped spelling found, not found, an entry whose twin
    is listed skipped, names without a letter, an incomplete listing, an unreadable
    directory. One test reads the real disk and expects what its platform gives: exact here,
    folded on the macOS and Windows legs;
  - the glob under both rules, `filter`, the zones through the checker, and the include
    guards through the checker, each under both rules;
  - the walk's excluded names under both rules, `path_under_excluded_dir`,
    `update_file` for a path under `Vendor/`, and `as_listed` in eight cases (both rules, an
    absolute path below and outside the base, a name the listing lacks, a name held in two
    cases, `..`, a link);
  - the session: its root's rule is what its platform gives, which is folded on the macOS and
    Windows legs.
- Checked here: seven mutations (the excluded-names comparison, the walk's rule, `update_file`'s
  rule, the zones' rule, the guards' keys, `ignoreErrors`' rule and `as_listed`) each fail
  tests; clippy; the project,
  app, lsp, ignore, restricted-symbol and checker tests; the CLI tests; the module graph and
  arch; the Windows type check, which covers `CaseRule` and its tests.
- Linux's output is unchanged: the probe gives exact here. Its cost is the root's listing, which
  the walk reads when the root is walked, and one `symlink_metadata`.
- Left to the legs: the folded rule on a real folding disk. The platform-conditional tests run
  there.

### R8. The include rule: what a run can find

**Now** (`checker/includes.rs`). The rule looks an include up as if the analysing machine were
the deployment.

- It looks in the project root as the working directory, in the including file's directory,
  and in an include path of Debian's `.:/usr/share/php` plus the values the checked file itself
  sets. A relative include path entry is taken as relative to the including file's directory.
- The platform is this machine's: `PATH_SEPARATOR` is `:`, a set value splits on `:` (on
  Windows `C:\lib` splits at the drive letter), `DIRECTORY_SEPARATOR` is the machine's and
  `get_include_path()` is `.:/usr/share/php`.
- **What PHP does** (php 8.5, the system calls read with `strace`):
  - A plain name is tried in each include path entry in order, then in the including file's
    directory, then in the working directory, whether or not the include path names it. A
    relative entry is relative to the working directory, and an empty entry is the filesystem
    root (`/name`).
  - `./name` and `../name` are tried in the working directory only: `./x.php` is not found
    in the including file's directory.
  - An absolute name is that file.
- So the rule answers from the analysing machine's disk where it cannot know the deployment's:
  - `require 'PEAR.php'` is reported or not by whether this machine has PEAR in
    `/usr/share/php`, which it does here;
  - `./x.php` is accepted when `x.php` is beside the includer;
  - an absolute path, or a `../` above the root, is judged by this machine's files.

**What a run can know.** Of an include, the project's files and the text of the code. The
include path is the deployment's php.ini value, with Composer's autoloader prepending its
packages' `include-path` entries, and then whatever `set_include_path()` calls ran, one of which
runs first. The working directory is the shell's for the CLI and the script's directory for a
web SAPI (from php-src: only the CLI is installed here). The platform is unknown, and so is
every file outside the project.

**Change** (decision 8).

1. **A value is reported when no run can find it.** Each value an expression can take is judged
   on its own, as now: `'a.php'|'b.php'` reports the missing one. A value is missing when no
   combination of the unknowns below finds a file in the project. A combination that leaves the
   project, or has a part unknown, counts as finding it.
2. **Two worlds.** An expression is read in a Unix world (`PATH_SEPARATOR` `:`,
   `DIRECTORY_SEPARATOR` `/`, `\` a character of a name) and in a Windows world (`;`, `\`, `\`
   a separator, a drive prefix, `.\` and `..\` relative). Found in either is found. A message
   names the value as the analysing machine's own world reads it: the Unix world's on Linux and
   macOS, and the Windows world's on Windows. `__DIR__` and `__FILE__` are the project's paths,
   each world spelling the place its own way. Classifying a name is a pure function of the
   world and the name.
3. **Where a value is looked for,** in PHP's order:
   - an absolute name is that file;
   - a `./` or `../` name is looked up in the working directory only;
   - a plain name is looked up in each include path entry, in the including file's directory
     and in the working directory.
4. **The include path** takes these values, and a plain name is looked up in their union:
   - the declared `include_path`, a new key in the deployment's syntax, split by each world's
     separator (`.:/usr/share/php`, `.;C:\php\pear`);
   - with the entries Composer's autoloader prepends: the `include-path` of the root package
     and of every installed package, under the package's directory;
   - or any value some analysed file sets with `set_include_path()` or `ini_set('include_path',
     …)`, where `get_include_path()` is the first kind of value.

   The include path is **open** when its start is undeclared (the deployment's php.ini is
   unknown), when a set value cannot be computed, and when an entry, or an empty entry, leaves
   the project. A plain name is not reported while it is open. Declaring `include_path = "."`
   closes the start.
5. **The working directory** is one of the declared `working_dirs` (a new key, relative to the
   config file) or, undeclared, the root and the directory of every analysed file: a CLI run
   from the root, or a web script's directory.
6. **Outside the project.** A target outside the trees the project reads (the root, `paths`,
   `scan_paths`, `stub_paths` and the vendor directories) is not judged: an absolute path
   elsewhere, a `../` above the root, a drive that is another machine's.
7. **`get_include_path()`** is the declared value. `PATH_SEPARATOR` and `DIRECTORY_SEPARATOR`
   are the world's.

**Where it lives.** The project builds the environment from the config, Composer's manifests
and every analysed file's set values, and hands it to the checker. An edit to a file's
`set_include_path` re-checks the files whose includes depend on it. The pure parts (the world,
the classification of a name, the candidate locations) take no filesystem and are tested as
functions.

**Test.**

- Classification and candidates per world, on every platform.
- The checker: `set_include_path` in another file; Composer's `include-path`; a declared
  `include_path`; `working_dirs`; the include path open and closed; `./` against a declared
  working directory; a target outside the project; both worlds' `DIRECTORY_SEPARATOR`; a
  Windows-written `\` in a literal.
- Linux's output changes where the old rule was wrong, and each fixture it changes is listed
  with the rule that changes it: a plain name only the system include path could resolve, an
  absolute path elsewhere, and `./` beside the includer under a declared working directory.
  The Piwigo and monolog `fileNotFound` lines are compared before and after.

**Built** (`7d5bddd4`, `06486da9`).

- **Three modules,** cut by what the project may depend on. The project builds the include
  environment, so what it reads may not be a rule.
  - `checker::include_lookup` (analyser) is the pure lookup. It reads no file system: whether
    a candidate is a file is a predicate its caller passes.
  - `checker::include_values` (analyser) holds the strings an operand can take in a world
    (what `includes.rs` held for one), and the walk that finds the calls the lookup depends
    on: the guard functions, `set_include_path()`, `ini_set()` and `ini_alter()` of
    `include_path`, and `chdir()`.
  - `checker::includes` (rules) judges each value.
- **Two worlds.** An operand is resolved once in each. `PATH_SEPARATOR`, `DIRECTORY_SEPARATOR`
  and `dirname()` take the world's, and `get_include_path()` is a placeholder that the
  environment expands to the declared value, so an operand that names the include path is not
  judged as a path. The two results are judged as pairs. A pair is skipped when either world
  guards it or finds it, and the message names the value as the machine's world reads it
  (round 3's follow-up; the Unix value before). A name is classified by a pure
  function of the world and its text: absolute (root, drive, share), `./` or `../`, plain, or
  not a path this analysis can see (a stream wrapper's URL, a drive-relative `C:x`).
- **PHP's order,** read with php 8.5. An absolute name is that file. `./x` and `../x` are
  looked up in the working directory alone. A plain name is looked up in each include path
  entry, then the includer's directory, then the working directory. A relative entry is
  relative to the working directory. An empty entry before another is the root of the file
  system, a trailing one names nothing, and PHP refuses an empty value (`set_include_path('')`
  fails), so a declared `""` is not known.
- **The include path** is the new `include_path` key (the deployment's syntax, split by each
  world's separator), the values the analysed files set, and the `include-path` directories of
  the root package and of every installed package, which Composer's autoloader puts in front
  (below the package's directory, a leading or trailing `/` trimmed). It is open while the key
  is undeclared, a set value cannot be computed, or an entry leaves the project. An open path
  judges no plain name. The Composer directories are known, so they do not open it.
- **The working directories** are the new `working_dirs` key (relative to the config file) or,
  undeclared, the root and the directory of every analysed file. A `chdir()` adds its
  directory when that is absolute and this machine has it. One that cannot be computed in
  either world, or that is relative or has no path here in this machine's world, opens the
  working directory, and no name that depends on it is judged. The other world's reading of
  the same call adds a directory only when this machine has it: otherwise it is that world's
  spelling of the same place.
- **Outside the project.** The trees are the root, `paths`, the scan and stub roots and the
  vendor directories that exist. A candidate outside them counts as found. So does one below
  an unbuilt directory: the `vendor` of a Composer root whose `composer.json` is here and whose
  `vendor` is not, whatever `no_composer` says, since `composer install` makes it. A `..` stays
  where it is written, because the file system takes it after the names before it
  (`link/../x` is not `x`). Only the test of whether a candidate lies in a tree folds it.
- **Incremental.** The environment is built once, on first use. `ini_set`, `ini_alter` and
  `set_include_path` set one mention bit and `chdir` another, so a file that names none of them
  is not read for values. The project builds the environment again when a file that sets a bit
  is edited, and when any file is added or removed, since the directories scripts run from
  change with the files. If the environment differs as a whole, the analysed files that
  include something are checked again (`FileFacts::has_global_or_include`; a `global`
  statement sets it too, which only widens the set), and no other file is. Applying settings
  and finishing a load drop it. That is the plan's "the files whose includes depend on it":
  only an include is judged against the environment.
- **Added to the plan's text,** each from what a run can know:
  - `ini_alter()`, `ini_set`'s alias, and `chdir()`, which moves the working directory that
    every relative name depends on;
  - the unbuilt `vendor` directories;
  - the `""` rule and the trailing empty entry, found by running php.
- **The cache.** The per-file cache is in a directory named by the build's source hash, so
  entries parsed before the new mention bits are never read.
- **Linux's output.**
  - 15 new fixtures cover the rules:
    - an undeclared include path, a relative entry, a value set in another file, `ini_alter()`,
      a value that cannot be computed, an entry outside the project, and Composer's
      `include-path`;
    - a declared working directory with `./x`, `./x` not looked up beside the includer, and
      `chdir()` to a known and to an unknown directory;
    - a built and an unbuilt `vendor`, an include outside the project, and the Windows
      world's `\`.
  - Six existing fixtures include a plain name, which is judged only once the include path is
    declared. They have an `include_path = "."` configuration now; the old rule judged them
    against Debian's include path.
  - `missing_paths_are_reported_per_construct` named `dirname(__DIR__, 1) . '/elsewhere.php'`,
    which leaves the project and so is no longer judged. It now names `dirname(__FILE__, 1)`,
    the file's own directory, and the message reads `<fixture>/elsewhere.php`.
  - `check-project vendor/monolog` loses six `fileNotFound` lines and gains none (25,951 lines
    to 25,945; 20,111 diagnostics to 20,105). One is an absolute path of the deployment
    (`require "/app/source/vendor/autoload.php"`, in the FrankenPHP test), and five are plain
    names judged against an include path that monolog's configuration does not declare
    (`lib/byte_safe_strings.php` and three more in `random_compat`, and `not-exists`). The
    file cache off, cold and warm give the same output for each binary, and a warm run writes
    no cache file.
  - The Piwigo oracle is byte-identical to the base (171,439 bytes, sha256 `8ff144c3…`), with
    an empty stderr.
- **Tests.**
  - The lookup, 62 on Linux, with the platform-conditional ones on their own platform: the
    classification of a name in each world, `dirname()` in each, the environment's building,
    and the search.
  - The checker's guard tests, with a helper that reads all the diagnostics under a root.
  - The project's nine: the configuration reaching the rule, the undeclared path, and the
    edit, `chdir()`, removal, open-value and new-directory cases in both directions. Their
    exact sets of re-checked files show that a file with no include is left alone.
  - One server test, the edit that sets the include path clearing a report in another file.
  - Composer's `include-path` of a root and of a package, and a malformed one.
  - The mention bits of the three functions that set the include path and of `chdir`.
- **Checked here.**
  - Clippy, `-D warnings`; the lib suite (1,337) and the fixtures (3,847); the integration
    tests; the module graph (423 modules, 13,013 references, 236 listed exceptions) and
    `arch`.
  - The Windows type check, which now covers the lookup and its tests.
  - Mutations, each failing tests: the hooks in the edit, the removal and the refresh, the
    Composer directories, the declared working directories, the unbuilt directories, `chdir()`,
    and the filter of files that include.
- **Left to the legs.** The Windows machine's branches of the lookup (a rooted path takes the
  project's drive, a drive or share is a real path) and their tests run on the Windows leg.

**Round 3's follow-up: the project's own paths in each world's spelling** (`e45a69c0`).

- **Now.** `__DIR__` and `__FILE__` held the machine's text in both worlds. On a Windows
  machine the Unix world read `D:\a\x` as a relative name with backslashes in it, a plain name
  that is not judged while the include path is undeclared, and the `dirname()` of it was `.`.
  Nine include fixtures failed on the Windows leg for it, and so did a lib test of the
  backslash. The lookup's Windows branches had never run: the Unix world's absolute paths
  named "a machine this one is not" on Windows, where the Windows world's rooted paths named
  the machine's own root on Unix.
- **Change.**
  - `include_lookup::spell` gives the text of a place of the project in a world. A Unix
    machine's paths are rooted paths in either world, as before. A Windows machine's are, in
    the Unix world, the names below the root of the project's drive; a place on another drive,
    or on a share other than the project's, has no spelling there, and an include built from
    it is not judged.
  - `native_absolute` takes a rooted path of either world to the root of the project's drive
    on Windows, as the Windows world's rooted path already was. The same code is judged alike
    on every platform: `/proj/lib/a.php` in a project at `C:\proj` names `C:\proj\lib\a.php`.
  - A report names the value as the machine's own world reads it. On Linux and macOS that is
    the Unix world's, as before; on Windows it is what the user's machine would hold
    (`D:\a\x\tools/gone.php`).
- **Tests.**
  - Pure, on every platform: `spell` for a Unix machine, a Windows machine, a share and a
    place on another drive, and that a spelled place reads back as the place.
  - Windows: a rooted Unix path on the project's drive, the project's own places judged in the
    Unix world, and `native_absolute`'s table.
  - The project: a report names `__DIR__ . DIRECTORY_SEPARATOR . 'gone.php'` with the
    machine's separator, and a path built from the file's own place is judged whatever the
    include path.
  - The fixture whose last line reported a `DIRECTORY_SEPARATOR` path loses that line: its
    spelling is the machine's, so no one expectation holds on every platform. The project test
    above holds it. The fixture harness names the checked file's directories below the
    fixture's (`<fixture>/tools`) as it names the fixture's own, so an includer in a
    subdirectory reads alike on every platform.
- **Checked here:** the lookup, project and LSP include tests on Linux (79) and, for
  `x86_64-pc-windows-gnu` under wine, the include tests (75) and all 3,847 fixtures, among them
  the nine of the Windows leg. Linux's output is by construction unchanged: the Unix machine's
  spelling is the text, and the message names the Unix world's value.

### R9. Discovery: unreadable spellings and listing misses

**Now** (`composer/src/vendor.rs`).

- On Windows, `path_of` (`:748`) turns bytes that are not UTF-8 into the empty path, though its
  doc says "read lossily". PHP for Windows opens paths as UTF-8, so such an entry names no
  file.
- A name its directory's listing does not hold is dropped (`entry_canonical`, `:729`). Composer
  copies `composer.json`'s spelling, so on a case-insensitive volume a spelling whose case
  differs from the disk's names the file.

**Change.**

- Bytes that are not UTF-8 give `None`: the entry names no file.
- A listing miss asks the system (`canonical::canonicalize`). That finds the file where the
  volume folds case, and fails at once elsewhere.

**Test.**

- A discovery test with a case-differing spelling: found on the Windows and macOS legs, absent
  here.
- On Windows, an entry that is not UTF-8.

On Linux a miss is an absent file. Piwigo's misses are counted before and after: no new
system call is expected.

**Built** (`da9e86df`).

- `path_of` returns `None` for bytes that are not UTF-8 where a path is text. The old reading
  gave the empty path, which resolved to nothing too, while its doc said "read lossily". The
  reading is `path_of_text`, which the tests compile on every platform, so Linux tests the
  decision the Windows build takes. A Unix name stays its bytes.
- A name its directory's listing lacks is resolved by the system, as a directory that cannot
  be listed already was. The system finds the file where the directory folds case, and where
  the listing is older than the file, and fails at once elsewhere. The plan placed the drop in
  `entry_canonical`, but a miss there leaves the directory unlisted, and the file then goes to
  the system. The drop was in `locate`, where the file's own name is looked up.
- Tests (four, in `vendor_context_discovery`):
  - a file written after its directory was listed is found, and one that neither the listing
    nor the system has is not;
  - a name, and a directory, spelled in another case name the file where the disk folds case
    (in the spelling the disk has) and nothing where it does not. The expectation is what the
    platform gives, as the case rule's probe test has it: found on the macOS and Windows legs,
    absent here;
  - bytes that are not UTF-8 name no file where a path is text.
- Checked here:
  - Two mutations (the miss dropped again, the lossy reading) each fail a test.
  - Clippy, `-D warnings`, and the 17 discovery tests.
  - The Windows reading of `path_of` type-checks, with its `cfg` flipped for one `cargo check`.
  - The misses on Piwigo and monolog, counted by their system calls: `strace -c` on the gate
    binaries of `7d5bddd4` and `da9e86df` gives the same `statx` and `readlink` counts for
    both (Piwigo 16,977 and 3,171; monolog 10,707 and 1,516). No miss occurs in either, so
    the change adds no call.
  - The output is unchanged: monolog's, off, cold and warm, and Piwigo's JSON (171,439 bytes,
    sha256 `8ff144c3…`).
- Left to the legs: the folding disks. The two case tests assert what a folding disk gives
  only on the macOS and Windows legs.

### R10. The program runs on a thread whose stack it chooses

**Now.**

- `syntax::stack::guard` grows a 16 MiB segment (`stack.rs:12`) when less than `RED_ZONE` (1 MiB,
  `:9`) is left.
- Windows gives the main thread 1 MiB, so every outermost guarded call there grows a segment,
  which on Windows is a fiber, and the next call does it again.
- `check`, `check-project`'s main-thread work and the language server's request loop all run on
  the main thread (`main.rs:96-119`).
- Linux's main stack is `ulimit -s`.
- `RED_ZONE`'s doc says it is "well under the smallest thread stack (2 MiB)". That holds for every
  thread but the main one.

**Change.** `main` does the following, in order:

1. It parses the command line.
2. It calls `keep_freed_memory` while it is the only thread.
3. It runs the command on a thread of `WORKER_STACK_SIZE` (`project.rs:373`).
4. It returns or exits with the command's outcome. A panic is resumed, so the exit status stays
   101.

The dhat profiler and hotpath's guard stay in `main`.

**Test.**

- R1's test on every leg.
- A unit test that the command thread's stack is the size asked for (`stacker::remaining_stack`).

On Linux nothing in the output changes; the cost is one thread spawn.

**Built** (`007d827b`).

- `main` parses the command line, keeps freed memory while it is the only thread, and runs the
  command through `on_worker_stack`: a scoped thread named `phpstan-rs-main`, with
  `WORKER_STACK_SIZE`, which `main` joins. `run` holds the old `match`. A panic of the command
  is raised again in `main` by `resume_unwind`, so the hook does not report it twice and the
  status stays 101. The dhat profiler and hotpath's guard stay in `main`.
- `RED_ZONE`'s doc names the smallest stack a walk runs on, a spawned thread's 2 MiB, and why
  the main thread is not one. The profiling notes say where the work is now: `CLAUDE.md`'s
  callgrind note and `scripts/perfdump.py`'s `<main tid>`.
- The test needs `stacker` as a dev-dependency (the workspace's pin, which syntax already
  uses). Both lockfiles pass `--locked`.
- Tests (three, in `main/tests/worker_stack.rs`): the thread's stack is the size asked for
  (`stacker::remaining_stack`), the body runs on another thread and returns its value, and a
  panic reaches the caller with its payload.
- Checked here:
  - Three mutations (a 1 MiB stack, no thread, a replaced panic) each fail a test.
  - Clippy, `-D warnings`, and the 44 CLI tests (R1's deep-nesting test among them), the LSP
    exit-code, version and file-cache tests.
  - With the main thread's stack limited to 1 MiB (`prlimit --stack`, Windows' size),
    `check-project vendor/monolog` makes 36,914 `mmap`, 36,878 `mprotect` and 36,869 `munmap`
    calls before, one set for each growth of the stack guard, and 65, 29 and 25 after. At the
    default 8 MiB it makes 63, 27 and 21 before, and 66, 29 and 26 after. On Linux the
    growth is a mapping, and no time difference shows: the four commands, five runs each in two
    rounds, give 5.6 to 6.5 s with a spread of 0.2 to 0.5 s, under a load average of 4.5 to 7.
    A growth on Windows is a fiber, which is not measured.
  - Monolog's output (off, cold and warm) and Piwigo's JSON are identical to the previous
    row's.
- Left to the legs: the Windows leg runs the CLI tests, R1's among them, with the 1 MiB main
  stack.

### R11. The caches where each platform keeps them, refreshed on Windows

**Now.**

- `cache_dir` (`registry/stub_cache.rs:34-45`) reads `XDG_CACHE_HOME` or `HOME`. On Windows
  `HOME` is usually unset, so the stub cache and the per-file cache are off, and every run builds
  the stub registry cold.
- `read_entry` (`project/file_cache.rs:313-326`) refreshes an entry's last use through the
  read-only handle it read the entry with. Windows refuses to set a time without write access,
  and the error is ignored (`let _`), so eviction there goes oldest-written first. The file's
  own test opens for writing (`:498`).

**Change.**

- `dirs::cache_dir()` joined with `phpstan-rs`. That is `$XDG_CACHE_HOME` or `~/.cache` on Linux
  (the passwd entry's home where `HOME` is unset), `~/Library/Caches` on macOS, and
  `{FOLDERID_LocalAppData}` on Windows. `PHPSTAN_RS_CACHE_DIR` and the `NO_*_CACHE` switches
  work as now.
- The refresh opens the entry for writing.
- The dependency is `dirs = "=7.0.0"`: MIT OR Apache-2.0. Its `dirs-sys` and `option-ext`
  (MPL-2.0) are allowed by `deny.toml`, and `windows-sys` 0.61.2 is already in the tree.

**Test.**

- The precedence (the override, the off switch, the default).
- On every leg, a refresh test: an entry older than `REFRESH_AFTER` is read, and its mtime
  moves.

On Linux nothing changes where `HOME` is set, and the oracle runs set the directory
explicitly. Where `HOME` is unset, the cache now comes on.

**Built** (`a94e27ec`).

- `cache_dir` is `PHPSTAN_RS_CACHE_DIR`, else `phpstan-rs` in `dirs::cache_dir()`, and none
  when `PHPSTAN_RS_NO_STUB_CACHE` is set. The choice is a function of its inputs
  (`cache_dir_of`), so its tests need no environment, and the platform is asked only when no
  directory is chosen.
- Two differences from the old reading, both on Linux and both what the XDG specification
  says. A relative `XDG_CACHE_HOME` is ignored, where the old code used it as given. And where
  `HOME` is unset, the passwd entry's home gives the directory, so the cache comes on.
- `refresh_last_use` opens an entry that is older than `REFRESH_AFTER` for writing, and sets its
  time through that handle. An entry that cannot be opened keeps its age.
- The dependency is `dirs` 7.0.0, the latest release, which brings `dirs-sys`, `option-ext`
  (MPL-2.0, which `deny.toml` allows) and `redox_users` (for Redox only). `libc` and
  `windows-sys` 0.61.2 are already in the tree. Both lockfiles hold the four packages and pass
  `--locked`; `cargo deny check` and `cargo machete` pass.
- Tests:
  - three on the precedence: the off switch beats a chosen directory, a chosen directory is
    used as it is and the platform is not asked, and the default is `phpstan-rs` in the
    platform's directory, or none when it has none;
  - the platform's directory follows its convention, with an arm for each system:
    `~/Library/Caches` on macOS, `XDG_CACHE_HOME` or `~/.cache` on Linux, and on Windows the
    folder `%LOCALAPPDATA%` names (compared as a file, since the environment may spell it
    with a short name);
  - three on the refresh: an entry older than the age moves to now, a recent one keeps its
    time, and an absent one is left absent.
- Checked here:
  - Four mutations (no refresh, a refresh of every read, the platform asked before the choice,
    the off switch ignored) each fail a test.
  - Clippy, `-D warnings`; `arch`.
  - The Windows arm of the convention test type-checks, with its `cfg` flipped for one
    `cargo check`.
- Left to the legs: the refresh test passes on Linux through either kind of handle, so the
  Windows leg is what tells the fix from the read-only handle's refusal; and the Windows and
  macOS conventions.

### R12. Colours on Windows consoles

**Now.** The code frame writes ANSI escapes whenever stdout is a terminal and `NO_COLOR` is unset
(`main/check_project.rs:264-266`, `output/codeframe.rs:16-24`). A Windows console prints them
raw unless virtual-terminal processing is on, and a program must turn that on itself.

**Change.** On Windows, colour is used only once `anstyle_query::windows::enable_ansi_colors()`
returns `Some(true)`. `anstyle-query` 1.1.5 is already in the tree through clap. Other platforms
are unchanged.

**Test.** The decision as a function of (terminal, `NO_COLOR`, enabled).

**Built** (`acc33609`).

- The plan tested `anstyle_query::windows::enable_ansi_colors()`. It is built on
  `anstream::AutoStream::always` instead, which uses that function and does more:
  - The function enables the mode on stdout and then on stderr, and reports failure when
    either fails. A terminal whose stderr is redirected (`2> log`) has the mode on for
    stdout and is still reported as having none, so a rule of "colour only on `Some(true)`"
    would drop its colour.
  - A console that has no such mode has no fallback in the plan's rule. `always` translates the
    escapes with the console API (`anstyle-wincon`) when stdout is a terminal, the mode could
    not be enabled and `TERM` does not claim ANSI. Otherwise it passes them on.
  - Elsewhere `always` passes the bytes on as they are.
- `anstream` 1.0.0 is a direct dependency. Clap already brings it, with `anstyle-query` and
  `anstyle-wincon`, so each lockfile gains one edge and no package. `cargo deny check` and
  `cargo machete` pass.
- `wants_colour(stdout_is_terminal, no_color)` is the decision: a terminal, unless `NO_COLOR`
  holds something (an empty value counts as unset). `print_friendly` writes the report, through
  `anstream` when it has colour and with `print!` when it has none.
- Tests: the decision's two tables, for a terminal and for what is not one, with `NO_COLOR` unset,
  empty and set. Two mutations (the terminal ignored, an empty value counted as set) each fail
  one.
- Checked here: clippy; the binary's 8 tests and the 44 CLI tests. Under a pseudo-terminal
  (`script`), the friendly report of the new binary and of the previous row's is
  byte-identical, with colour (766 bytes, 14 lines with escapes), with `NO_COLOR=1`, and piped.
- Left to the legs, and to a person: neither the enabling nor the translation runs on a CI
  leg, because the runners' stdout is a pipe. A Windows console is where the report has to be
  looked at.

### R13. The tests on every platform

**Change.**

- **Links.** A test helper `link(original, link)`:
  - Unix: `symlink`.
  - Windows, for a directory: a junction, which is what Composer makes for path repositories
    and needs no privilege. It uses the `junction` crate as a `cfg(windows)` dev-dependency,
    whose `unsafe` stays a dependency's.
  - Windows, for a file: `symlink_file`. That needs the symlink privilege, which the runners
    have; without it the helper fails, saying to enable Developer Mode.
  - The link tests drop `#[cfg(unix)]`: in `walk_identity.rs`, `session_identity.rs`,
    `symlink_identity.rs`, `workspace_folders.rs`, `manifest.rs`, `dir_listings.rs`,
    `package_ownership.rs` and `vendor_context_discovery.rs`. Std counts a junction as a link
    (`FileType::is_symlink` holds for name-surrogate reparse points), so the walk should need no
    Windows branch; the tests check that.
- **`hard_links_stay_two_files`** drops its `cfg`: `fs::hard_link` is portable.
- **The permission tests**: `a_directory_that_cannot_be_read_has_no_listing`,
  `a_php_file_its_directory_cannot_list_is_kept`,
  `a_file_the_tree_cannot_place_keeps_the_listing_in_path_order`, and the oracle tree's
  `set_locked`.
  - They lock a directory with a Unix mode, so they stay unix-only. The code reads
    `read_dir`'s error alike everywhere.
  - The oracle tests split: their link part runs everywhere.
- **CRLF.** A Windows checkout of a user's project is CRLF by default. New tests run the lexer,
  `phpdoc`, `LineIndex` and the LSP positions over CRLF sources: line numbers, columns, heredoc
  terminators and doc tags.
- **Round 1's Unix assumptions.** Every test that round 1 shows to assume Unix spellings is
  fixed:
  - The fixture harness prints the path column with the relative path's components joined with
    `/`. `portable` names the fixture's directory in the spelling the messages use, the
    canonical one.
  - `psr4_locations_are_checked` gets a layout every filesystem holds. The correctly placed
    class moves out of `src/Http/`, so `src/http/` is the only spelling of that directory, and
    the case mismatch is reported on every leg.
  - `the_entries_are_sorted_by_name_bytes` keeps its order (a capital, then `_`, then small
    letters) with names that differ in more than case.
  - A test URI is made by `path_to_uri`, not by `format!("file://{}", …)`.
  - A test that compares the walk's paths compares them with the canonical form of the paths
    it wrote (`collect_php_files_…`).
  - R5's extension fixes the LSP tests that compared canonical URIs with the temp directory's
    spelling, without changing them.
- **Round 2's.**
  - `a_superseded_run_sends_nothing_and_the_newer_run_sends_its_result` looks up
    `Server::last_diagnostics` by the canonical path the server keys it by.
  - The test files whose tests are all `cfg(unix)` leave imports and helpers unused on
    Windows. Once their link tests run everywhere, those imports and helpers are used again.
    Whatever stays unix-only, the permission tests, has its imports under the same `cfg`.

**Built** (`f5e418d6`, `ee903695`, `2c7bf898`, `df4be250`).

- **Round 1's and 2's Unix assumptions** (`f5e418d6`, `ee903695`):
  - the fixture harness spells the path column with `diagnostics::report_path`, and
    `portable` names a fixture's directory in the project's canonical spelling, and in the
    spelling the harness was given;
  - the PSR-4 fixture has one spelling of each directory: the correctly placed class moved to
    `src/Controller.php`, so `src/http/` is the only spelling of that directory, and
    `psr4.caseMismatch` is reported on a folding disk too;
  - four tests name a path as the system spells it: `collect_php_files_…`,
    `the_entries_are_sorted_by_name_bytes` (names that differ in more than case),
    `a_superseded_run_…` (keyed by the canonical path) and the parser-crash session test (a URI
    made by `path_to_uri`).
- **Links** (`2c7bf898`):
  - `test_links` is a test-only module in composer and in the package, each with `link_dir`
    and `link_file`; the package's has `unlink_dir` as well. A directory link is a symbolic
    link on Unix and a junction on Windows (the `junction` crate, a Windows-only
    dev-dependency, `=2.1.0`, MIT; its `unsafe` stays a dependency's). A junction holds an
    absolute target, so a relative one is joined to the link's directory first, and the
    junction crate resolves its `..`. A file link is a symbolic link on both; its relative
    target is written with `\`, which is the only separator Windows reads there. A privilege
    error (1314) is reported with the Developer Mode hint.
  - 21 link tests lost their `#[cfg(unix)]`, in `walk_identity.rs`, `session_identity.rs`,
    `symlink_identity.rs`, `workspace_folders.rs`, `path_spelling.rs`, `manifest.rs`,
    `dir_listings.rs`, `package_ownership.rs` and `vendor_context_discovery.rs`; so did
    `hard_links_stay_two_files`. The permission tests stay Unix-only.
  - The oracle tree's unreadable directory is made so by `set_locked` where a mode can do it.
    Elsewhere, and for a user who can read a mode-000 directory, the directory is listed, and
    the test expects `secret.php` then. It used to return early for the second case.
  - The unused imports and helpers the Windows leg found are used again: the tests that used
    them run there now.
  - Checked here: clippy and the tests of composer (76) and of the package's converted
    modules (66); both lockfiles `--locked`; `cargo deny`; `cargo machete`; the module graph
    and arch gates; the Windows type check of both helpers, with the Windows arm of each
    function compiled for `x86_64-pc-windows-gnu`. The Windows leg runs the junctions.
- **Line breaks** (`df4be250`):
  - **What the tests found.** The plan asked for CRLF tests of the lexer, `phpdoc`, `LineIndex`
    and the LSP positions. Run over `\n`, `\r\n` and `\r` alike, they found two defects older
    than the plan, on every platform.
    - A `//` or `#` comment in a CRLF file kept its `\r`: its span and its text ended after it.
    - A lone `\r` was not a line break anywhere. PHP counts it (php 8.5: `__LINE__`, the end of
      a `//` comment, the newline a `?>` takes, the lines of a heredoc, and so the label that
      closes one), and so do VS Code and the LSP specification ("`\n`, `\r\n` and `\r`"). A
      file with only `\r` ended lines had its rest after the first `//` comment swallowed, and
      no diagnostic at all.
  - **One definition.** `syntax::span` holds `line_break_len` (the bytes of the break at the
    start of a slice: two for `\r\n`, one for either alone), `lines` (as `str::lines`, a
    lone `\r` ends a line too) and `count_line_breaks`. `LineIndex` uses the same rule: a line
    ends at `\n`, `\r\n` or `\r`, and the `\r` of a `\r\n` is part of its break. Its
    `line_end` is the offset before the whole break, so the position of an LSP client that
    names a column past the visible end finds the end of the line (the protocol's clamp),
    where it used to land between `\r` and `\n`. A bit for each line records which lines end in
    `\r\n` (nothing for a source without one), and `line_break()` is the first break the
    source writes. A source with no `\r`, which is every checkout on Linux and macOS, is
    scanned for `\n` as before, after one `memchr` pass for `\r`; the scan for all three runs
    only for a source that holds one.
  - **Where it is read.**
    - The lexer: a line comment ends at any break (and so its text has none), `?>` takes one
      break of any of the three, and a heredoc or nowdoc reads its opening line, its lines
      (the label closes only at the start of a line), the break before the label and its
      indentation stripping by the same rule. A break inside the body stays as the source
      wrote it.
    - `phpdoc`'s two parsers split a docblock with `syntax::span::lines`.
    - `@phpstan-ignore` comments: the lines a comment spans, and the identifier list's
      lines.
    - The `friendly` format's frame splits the source with `lines`.
    - `composer.json`'s line numbers (`LineFinder`) come from a `LineIndex`.
    - An auto-fix that adds a line ends it with the file's own break:
      `constructor.missingParentCall`, `parentCall.missing`. A file that mixes breaks gets the
      first one it writes. Linux files get `\n`, as they did.
  - **Checked against PHP.** php 8.5 gives the same `__LINE__`, heredoc contents (hex-compared
    for six heredocs) and close-tag newline for each of the three breaks, and the new tests
    assert what it gave.
  - **Tests.**
    - The lexer, in `lexer/tests/line_breaks.rs`: comments, `?>`, heredoc and nowdoc bodies,
      the label rules, indentation, empty and blank-line bodies, interpolated parts' positions,
      and every token's line and column across a source of all kinds, each over the three
      breaks. The skipping test's byte-by-byte oracle ends comments at `\r` too.
    - A randomized one: 3,000 sources of up to eight fragments (comments of each kind,
      strings, heredocs, nowdocs, close tags, blank lines), each written with the three
      breaks, lex to the same kinds of token between the same lines and columns.
    - `LineIndex`: lines, starts, ends, positions and clamping over the three breaks, a `\r`
      before a `\r\n`, the first break of a source, `lines` against `str::lines`, and the scan
      of a source without a `\r` against the general one.
    - `phpdoc`: a docblock with every tag kind, a wrapped array shape and its `//` comments
      parse equal across the three breaks, in both parsers.
    - The package: ignore comments (lines, identifiers, errors); the friendly frame; the two
      auto-fixes and the member deletion; `composer.json`'s lines; and the LSP, where a
      document's diagnostics, definition and hover (and one at the end of a line) are equal.
    - Mutations: the old line splitting in phpdoc fails five tests, and the old comment end
      fails three of the lexer's, the randomized one among them.
  - **Cost.** `LineIndex::new` runs once per file, so its scan is in every run's instruction
    count. Counted by callgrind over Piwigo's 20,200 files (91 MB, 2.54 M lines), the scan
    alone with the file reads taken out:
    - the old `memchr_iter` for `\n`: 147.7 M instructions;
    - one `memchr2_iter` for both breaks: 198.4 M (+50.7 M), since lines of about 36 bytes make
      each call dear;
    - one pass for `\r`, then the old scan: 159.9 M (+12.2 M), which is what the code does.

    The time is the same in each (39 ms for the corpus). The lexer's `memchr3` for a comment's
    end and the docblock splitting are not counted apart: the batch's callgrind run counts the
    sum.
  - **Corpora.** Seven vendored corpora (guzzle, amphp, reactphp-http, phpstan-strict-rules,
    composer, doctrine-collections and wordpress: 6,142 PHP files, 43,886 diagnostics),
    checked as they are, with every line break written `\r\n`, and with every line break
    written `\r` (a file that held `\r\n` first read as one break). Each is `check-project
    --no-composer --format json` with the file cache off, and the diagnostics are compared
    one by one, from a gate binary built from the commit.
    - `\r\n`: identical, but for one message that quotes a string literal with its own breaks
      (doctrine-collections, `DoubledMethod.php:223`: `"\n\n …"` is `"\r\n\r\n …"`).
    - `\r`: the same, and 17 lines that differ besides, all from regular expressions with the
      `x` flag and `#` comments (wordpress `upgrade.php:3067` and `class-snoopy.php:630`, and
      `UriValidator.php` in composer's vendor tree). PCRE ends such a comment at `\n` alone,
      so with `\r` the comment runs to the pattern's end, its groups are gone, and the tool
      reports a pattern that does not compile or an offset that does not exist. php 8.5
      compiles the wordpress pattern with `\n` and with `\r\n`, and with `\r` fails with
      "missing closing parenthesis at offset 931", the message and offset the tool reports.
  - **Checked here.**
    - Clippy, `-D warnings`, on the workspace; the lib suite (1,351), `syntax` (300), `phpdoc`
      (157) and `composer` (77); the fixtures (3,847, gate profile).
    - The corpora above.
  - **Left to the legs.** The junction and file-link tests on Windows, and `set_locked`'s
    fallback where a mode cannot lock a directory. The line-break tests build their sources in
    memory, so they run alike on every leg.

### R14. Build scripts: the same embedded data everywhere, and dhat's walk per platform

**Now.**

- `stub-sources/build.rs` `write_path_array` (`:43-50`) spells each embedded stub's path with
  `to_string_lossy`, which gives `standard\basic.php` on Windows. The checked-in `index.tsv`
  spells it `standard/basic.php`, so the `stub_index` test cannot pass there, and the
  provenance indices differ by platform.
- `include_str!` embeds a CRLF checkout's bytes as they are (R2).
- `patches/dhat` builds the frame-pointer walk for any x86_64 target given the flag (`build.rs`),
  but the walk calls glibc's `pthread_getattr_np` (`lib.rs:1293-1300`). A frame-pointer dhat
  build for Intel macOS or Windows does not compile, and aarch64 never takes the walk.

**Change.**

- The paths are joined with `/`.
- A source that holds `\r\n` is embedded from an LF copy in `OUT_DIR`.
- dhat:
  - Linux: the walk reads `rbp` on x86_64 and `x29` on aarch64, with `pthread_getattr_np`.
  - macOS: the walk on both architectures, with `pthread_get_stackaddr_np` and
    `pthread_get_stacksize_np`.
  - Elsewhere, Windows included, the unwinder.
  - `build.rs` and the README say so.

**Test.**

- `stub_index` on every leg.
- R2's leg step: `cargo check -p phpstan-rs --profile dhat --features dhat-heap` with
  `-C force-frame-pointers=yes`.
- Once on the macOS leg: a dhat-heap run on a small project whose stacks are whole (recorded).

**Round 2.** The dhat build warns, on Linux too, of two imports it leaves unused:
`build_declarations` (`registry.rs:78`) and `project/declared_classes.rs:62`. Each takes the
`cfg` of the code that uses it, so the dhat check builds without warnings.

**Built** (`92074e5a`, `4ce90ca9`, `08315491`).

- **The embedded data** (`92074e5a`):
  - `stub-sources/src/embed.rs` holds the two decisions the build script takes. The script
    includes the file by path and the crate's tests include it, so both run the same
    functions. It is under `src/` because `PHPSTAN_RS_SOURCE_HASH` hashes each crate's `src`,
    `Cargo.toml`, `build.rs` and `index.tsv`, and the hash has to follow it.
  - `slash_spelling` joins a path's components with `/`: `standard/basic.php` on every
    platform, as `index.tsv` spells it.
  - `with_lf_breaks` is the inverse of a checkout's `\n` to `\r\n`: it drops the `\r` of each
    `\r\n` and nothing else, and gives nothing for a source that holds none. The script embeds
    that copy, written under `OUT_DIR/lf/`, and every other source from where it lies. The
    data embedded is then the repository's, whatever `core.autocrlf` did to the checkout, the
    phpstan-src submodule's stubs included, which `.gitattributes` does not reach.
  - On Linux the generated file is byte-identical. With every vendored source converted to
    `\r\n`, the paths, the extensions, the 2,556 sources and the index of a build hash to the
    value of the LF checkout's.
  - Tests, in the crate: the slash spelling (and, on Windows, the spelling of a `\` path); the
    conversion's cases (pairs, a `\n\r\n` run, a `\r\r\n` that keeps its first `\r`, a lone
    `\r`, and no pair at all); and one that holds the embedded arrays to what every platform
    embeds: equal lengths, no `\` in a path, every path a `.php` file, and no source with a
    `\r\n`. The package's `the_checked_in_stub_index_is_up_to_date` compares the index made
    from the embedded data with `index.tsv`. It failed on Windows in rounds 1 and 2, and
    runs on every leg.
- **The two items only a debug build reads** (`4ce90ca9`): `registry::build_declarations` is
  read by the debug assertion that compares a file's kept declarations with the ones its tree
  builds, and `project::declared_classes` by the one that compares the classes found beside
  the declarations with a walk of their own. Each takes the `cfg` of its only user. The gate
  and dhat profiles have no debug assertions, and built with both warnings. The dhat build is
  clean now.
- **The heap profiler's walk** (`08315491`):
  - `build.rs` names the targets whose frame register and stack bounds the walk reads: x86_64
    and aarch64, on Linux and on macOS, when the build's flags hold `-C
    force-frame-pointers=yes`. Any other build, Windows included, unwinds with `backtrace`.
  - A frame record is the caller's frame pointer, then the return address, on all four
    targets. The walk reads `rbp` on x86_64 and `x29` on aarch64, and bounds itself with the
    thread's stack: `pthread_getattr_np` on Linux, and on macOS `pthread_get_stackaddr_np`
    (the high end) with `pthread_get_stacksize_np`. std keeps frame pointers in its non-leaf
    functions on these targets (read from the disassembly).
  - `patches/dhat` is outside the workspace, so it has a lockfile of its own, pinned to the
    root's versions. `lint`'s lockfile step runs `cargo metadata --locked` on it, and
    `.gitignore` covers its `target`.
  - Tests, in the crate, with the flag and without it, so that each walk has them: a backtrace
    taken under 40 open calls holds a frame for each, on a test thread and on a thread of its
    own; and, where the walk is built, a thread's stack bounds hold the addresses of its
    locals. The step `The dhat tests` runs them (`cargo test --locked --lib --manifest-path
    patches/dhat/Cargo.toml`, with the flag) on the Linux `test` job and on each platform's
    `platform-test` job, beside R2's `The dhat build` (`cargo check -p phpstan-rs --profile
    dhat --features dhat-heap`, with the flag) on `platform-check`. The plan's one recorded
    dhat run on the macOS leg is not made: a test keeps what the run would have shown.
  - Checked here:
    - a dhat-heap `check-project` run over a 79-file project gives 65,819 program points,
      each with a stack from the allocator's frame to the thread's start (`clone3`, or
      `_start` for the main thread), 23 to 340 frames deep, and the build has no warning;
    - the crate type-checks with the flag for x86_64 and aarch64 Linux, x86_64 and aarch64
      macOS, and x86_64 Windows, with the walk built for the first four and not the last;
    - reading `rsp` for `rbp` fails both backtrace tests.
- Also checked here: clippy on the workspace, `-D warnings`; the module graph and `arch`;
  both lockfiles and the dhat one with `--locked`; `cargo deny` and `cargo machete`.
- Left to the legs: the aarch64 Linux and macOS walks and their tests (no leg runs x86_64
  macOS, which is type-checked only), the Windows unwinder's, and the embedded data's equality
  on Windows.

### R15. Developer tooling

**Change.**

- `.cargo/config.toml`: `-Z threads=8` moves to `[target.'cfg(all())']`, so every host gets
  the parallel front end. mold stays x86_64 Linux's.
- xtask:
  - `touch` (`main.rs:1263`, `:1291`) becomes `File::set_modified`.
  - `diff -u` (`:935`) becomes `similar`'s unified diff, a dependency of xtask.
  - The perf gate's peak RSS (`:1118`) uses GNU `time -f %M` on Linux and BSD `time -l` on
    macOS, which reports bytes. On Windows the gate says that it has no peak-RSS reading, and
    hyperfine's timing still runs.
  - `arch`'s `is_engine_source` (`arch.rs:75`) matches the path below `src` by its
    components. It splits the text on `/`, so on Windows the exempt files and the `tests`
    directories never match.
- `scripts/`: each script that reads `/proc`, `perf`, valgrind, THP or `systemd-run` says in
  its usage that it runs on Linux.
- README's "Building from source" adds:
  - the pinned nightly (`rust-toolchain.toml`);
  - clang and mold on x86_64 Linux (`.cargo/config.toml`), and how to build without them;
  - where the caches are on each platform.
- README's "Installing a pre-built binary" verifies the checksum on each platform: `sha256sum -c`
  on Linux, `shasum -a 256 -c` on macOS, and PowerShell's `Get-FileHash` on Windows, whose
  output is compared with the `.sha256` file's hash.

**Built** (`7d7f2ce5`, `04a78a8b`, `b2528e33`).

- **`.cargo/config.toml`** (`7d7f2ce5`): `-Z threads=8` is in a `[target.'cfg(all())']`
  table, so every host compiles with the parallel front end. x86_64 Linux's table keeps the
  clang linker and mold. Cargo joins a triple table's flags before a cfg table's, so the flags
  x86_64 Linux builds with are the same, in the same order (`-C link-arg=-fuse-ld=mold -Z
  threads=8`), and nothing there is rebuilt.
- **xtask** (`04a78a8b`):
  - `touch` is `File::set_modified`, and `diff -u` is the unified diff of `similar` (3.2.0,
    Apache-2.0, a dependency of xtask alone): the sides labelled `old` and `new`, three lines
    of context and the no-newline marker, as `diff -u` writes them.
  - The perf gate's `peak_rss_kb` reads GNU `time -f %M` on Linux, in KB, and BSD `time -l` on
    macOS, in bytes, from the line that names the maximum resident set size. Windows has
    neither: the gate says there is no reading, still times the run, and compares no memory.
  - `arch`'s `is_engine_source` reads the path below `crates/phpstan-rs/src` by its
    components. The exempt files (`project/session.rs`, `main.rs`, `lsp.rs` and what is below
    `main/` and `lsp/`) and the `tests` directories matched on `/` alone before.
  - Tests (xtask's 19): both `time` formats, the diff's format with its markers, `touch` (it
    moves a file's time to now, leaves its contents and fails on an absent file), and the
    engine rule for paths built with the platform's separator, with a `\` spelling on Windows.
- **README and scripts** (`b2528e33`):
  - "Installing a pre-built binary" verifies the checksum with `sha256sum -c` on Linux and
    `shasum -a 256 -c` on macOS, and on Windows with PowerShell's `Get-FileHash` compared to
    the `.sha256` file's hash. The Windows asset is a `.exe.gz`, which `GzipStream` unpacks.
  - "Building from source" says that the phpstan-src submodule has to be initialised, that
    the pinned nightly comes from `rust-toolchain.toml`, that x86_64 Linux links with clang
    and mold, and the command that links with the system linker instead (checked with
    `cargo build -v`). It says where the stub and per-file caches are on each platform, and
    the variables that move or turn them off.
  - Each script that reads `/proc`, records with `perf` or needs either's output says Linux
    in its usage line: `abab.py`, `cpugrowth.py`, `mimalloc-runs.sh`, `perfdump.py`,
    `scaling.sh` and `thp-probe.py`.
- Checked here: clippy on the workspace, `-D warnings`; the xtask tests; the module graph and
  `arch`; both lockfiles with `--locked`; `cargo deny` (for `similar`) and `cargo machete`.
- Left to the legs: `touch`, the diff and the engine rule on macOS and Windows, which run
  with xtask's tests; the macOS reading of `time -l`, whose parser is tested against a report
  written in that format and not against a Mac; and the PowerShell snippet.

### R16. `path_bytes` reads Windows paths from their bytes

**Change** (decision 2).

- **The plain forms on Windows:**
  - absolute: `X:\` followed by names separated by `\`. Every loaded path has this form after
    R3;
  - relative: names separated by `\`, the first of which is not a drive spec.
  - A name is non-empty, holds neither separator, and is not `.`; `..` reads as `ParentDir`, as
    on Unix.
- **How the pieces are cut:**
  - A sub-path is cut from `path.to_str()`, a scan for surrogates; a path that is not UTF-8 goes
    to std.
  - The prefix component comes from std, since `PrefixComponent` has no public constructor.
  - `hashed_ancestors` gives a prefix no ancestor of its own, as `Path::ancestors` gives none.
- **Left to std:** a `/`, `\\?\`, UNC, device and drive-relative paths.
- **`is_php_name`** reads a name's bytes on every platform.

**Test.** A Windows mix is checked against std function by function: each prefix kind in either
case, `\` and `/`, doubled and trailing separators, `.` and `..`, `a:b`, and non-ASCII names. It
sits beside the Unix mix. Each mix runs on its own platform, with its own count showing that the
bytes read most of it.

**Measure.** A criterion bench, `crates/path-bytes/benches/path_bytes.rs` (R18 makes the crate), times `path_bytes`
against std on a load's mix of paths. It runs here and on the Windows leg. The per-call times
go into LOAD-8.7; no load-level Windows figure exists.

**Built** (`9c555663`).

- **The reader,** `path_bytes::windows`, answers `file_name`, `parent`, `split`,
  `has_file_name`, `starts_with`, `strip_prefix`, `is_normal_absolute` and `components`. The
  plan said to cut a sub-path from `path.to_str()`, a scan of the whole path for surrogates.
  A part is checked where it is cut (`str::from_utf8` of the slice) by the reads of a name
  and a directory, and `components` checks the whole path once.
  - **Plain** is a drive and its root (`C:\`) followed by names separated by `\`, or names
    separated by `\` whose first is not a drive. A name is not empty, is not `.`, and holds
    no `/`; `..` is a `ParentDir`.
  - **Left to `Path`:** a path that starts with a separator (rooted, share, device, verbatim),
    a drive without a root (`C:a`, `C:`), a name that is not Unicode, and a `.` or empty
    component where the answer depends on it.
  - **A `/` is a separator,** which the plan's "left to std" left out: the reads that end at a
    separator use both, a path that holds one is plain only where the answer does not depend
    on it, and a path is compared with a base that spells `\` for the path's `/`. The drive
    letter is compared in either case and the names as they are.
  - **`a:b` is not a name.** `PathBuf::push` takes a name that starts like a drive for a path
    of its own and makes it the whole buffer, so `is_normal_absolute`, which says that
    rebuilding the path from its components gives it back, is false for such a path.
  - **The prefix** comes from `Path` (`PrefixComponent` has no constructor), from the two
    bytes of the drive, and it ends no ancestor: `Path::ancestors` of `C:\a` is `C:\a` and
    `C:\`. `hashed_ancestors` hashes the prefix and pushes no entry for it.
  - `is_php_name` read a name's bytes on every platform already.
- **Tests,** 15 in all, 13 of which run off Windows: the six of `tests.rs`, which hold the
  platform's reader to `Path` where they run, and nine of the Windows reader.
  - The Windows reader's tables (names and directories, prefixes and what is left after them,
    plainness, normal form, components, parts that are not Unicode) run on every platform
    against the answers Windows' `Path` gives, written down.
  - On Windows alone, every table is compared with `Path`'s own answers, and a generated mix
    must be read and not left to `Path` in most cases: 150 or more names, 100 or more
    directories, 5,000 or more stripped prefixes and 50 or more normal paths.
- **Verified here,** which the plan thought impossible, with MinGW and wine for
  `x86_64-pc-windows-gnu` (`scripts/winrun.sh`): the 15 tests on `path_bytes`' real Windows
  `Path`, composer's 84 of 89 (the 5 are the links wine cannot make), the package's lib tests
  but its links, all 3,847 fixtures, and clippy over the workspace. Wine found two defects of
  the reader before it was committed (a path that spells `/` against a base that spells `\`,
  and `a:b` taken for a plain name) and several expectations of the tests that Windows'
  `Path` does not give, among them the quirk of `push` above.
- **The bench** runs a query over a load's mix of 1,000 paths (absolute, below a root, a few
  directories deep, a PHP file) and over a mix the bytes leave to `Path` (a `.` component or a
  doubled separator). The `platform-test` job runs it on the three legs, so the log has each
  platform's numbers; LOAD-8.7 has them.
- Checked here: clippy on the workspace, `-D warnings`, for Linux and for
  `x86_64-pc-windows-gnu`; the lint job's other steps; both lockfiles and the dhat one with
  `--locked`; `cargo deny` and `cargo machete` (the bench's `criterion` is a dev-dependency
  already in the tree).
- Left to the legs: the MSVC build's run of the 15 tests, the links, and the timings on the
  runners.

### R17. The records

(Written while round 4 runs; each item's state is in the list.)

- `path_bytes`' module doc: which platforms read bytes, and how Windows' grammar differs.
  Done with R16.
- LOAD-8.7:
  - −126.7 M is Linux's, by callgrind;
  - macOS takes the same code, unmeasured;
  - Windows has R16's per-call bench.
- The memory `project-is-cross-platform`: the legs now test all three platforms.
- `WORK_STACK.md`: level 9 for this plan when it starts (the plan is copied to
  `CROSS_PLATFORM_PLAN.md`). It is deleted when the plan finishes, and level 8 resumes.

## Verification

**Per row.** `cargo fmt`, then `cargo check`, `cargo clippy -D warnings`, and the row's tests
(`-p phpstan-rs-composer`, `-p phpstan-rs --lib <module>`), in the debug profile with
`TMPDIR=/home/torres/.cache/prs-profile/fixes/tmp`. No full suite per row.

**Per round** (each push asked). The legs:

- `platforms`: macOS, Windows, aarch64 Linux;
- `gates`: Linux;
- `extension-ci`: three OSes.

A failure is read from the job's log, saved under `~/.cache/prs-profile/fixes/logs/`.

**At the end of the batch, on Linux:**

- `cargo test -p phpstan-rs --lib`, `--test fixtures` and the integration tests;
- `check-project vendor/monolog`, stdout and stderr byte-compared between a worktree at
  `791a57c6` and HEAD, both `LC_ALL=C sort`ed, off, cold and warm;
- the Piwigo oracle (171,439 bytes, sha256 `8ff144c3…`), off and warm;
- one off-mode callgrind run: the total within MEAS-3's spread (±20 M) of `8a1ca1e0`'s
  20,373,086,199, since R3, R4, R7 and R10 touch Linux's load path.

Builds and runs need no approval. One heavy job at a time, under `nice -n 19`, with
`/proc/loadavg` logged.

**At the end of the batch, elsewhere:**

- every leg green: clippy, the tests and the fixtures on Windows, macOS and aarch64 Linux;
- the smoke round trip on Windows;
- `release.yml` by `workflow_dispatch` for all five targets, every binary run once (asked).

## Not in this plan (checked)

- **The extension's TypeScript** already handles the platforms: `.exe` and `.cmd`, chmod, the
  quarantine attribute.
- **fnmatch's `?` and `[…]`** in `ignoreErrors` are a parity gap on every platform, not a
  platform-scoped one. PHPStan's matcher never sees a `\`: `normalizePath` has already turned
  each one into a separator (R6).
- **The release target scope** is not changed here: the glibc floor, musl, Windows ARM64.
- **`.cargo/config.toml`'s mold and clang** stay x86_64 Linux's. They are a link-speed choice;
  R15's README covers them.
