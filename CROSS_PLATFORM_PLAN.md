# The code scoped to Linux: the cross-platform fixes

## State (2026-10-18)

- **The plan is approved** (2026-10-18), with the four decisions below. `WORK_STACK.md`'s level 9
  tracks it.
- **Built:** R0 (`4f6bc51a`), R1 (`ce593a47`), R2 (`d8de9530`), R3 (`d360a4df`), R4
  (`c6902324`), R5 (`52844b07`), R18 (eight commits, `0e908059` to `80b3bbd9`) and R5's
  round-2 follow-up (`8d81da46`). The rows are built in the order of the table below, one
  commit each, with a pathspec, since another session shares the checkout. R18 has one commit
  for each step.
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
- **The module graph is fixed in this batch** (the user's decision with round 2's push): R18,
  built before R6. The 20 forbidden edges are refactored away, not listed as exceptions, and
  the graph passes. Round 2 reached the gates job's later steps, and R18 fixes their four
  failures too.
- **Next:** R6.
- **For the user:** R8's question, and the two findings under R5's round-2 follow-up (the
  disabled server still publishes on an edit, and disabling clears only the open files).

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

## How the rows are designed and judged

- **The most proper fix, not the smallest.**
  - One function per concern.
  - A lint enforces it where it can: `disallowed-methods` for `canonicalize`.
  - Each `cfg` branch is correct on its own platform, not merely compiling.
- **Linux is the control.** Every row leaves Linux output byte-identical unless the row names a
  Linux defect it fixes. The end of the batch proves it with the oracle, monolog and one
  callgrind run.
- **Windows and macOS are verified only on the CI legs (R2).** No local cross-build is possible
  here: aws-lc (rustls), the bundled SQLite, mimalloc, PCRE2 and stacker's assembly need the
  target's C toolchain and SDK.
- **No saving without a measurement.** R16's Windows reads are timed per call by a criterion
  bench on the runner. No load-level Windows figure can be made, and none is claimed.
- **Working rules:**
  - One commit per row, with a pathspec, since another session shares the checkout.
  - `cargo fmt` first, then check, `clippy -D warnings` and the row's targeted tests in the
    debug profile, with `TMPDIR` under `~/.cache`.
  - A push only after asking.
  - Comments carry no task references and no narrative.

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
| 7 | R6 | report paths spelled with `/` where PHPStan does, matched by components | Windows | R3 |
| 8 | R7 | the case rule, probed per root | Windows, macOS, Linux casefold | R6 |
| 9 | R8 | the include rule's platform values | Windows | R3 |
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
- round 3 after R13 and R14;
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

- A `platforms` job in `rust-ci.yml`:
  - Matrix: `macos-26` (arm64), `windows-2025-vs2026` (x64) and `ubuntu-24.04-arm` (aarch64
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
  - The aarch64 Linux leg builds natively on `ubuntu-24.04-arm` and runs its binary. This
    replaces the cross toolchain step. The image's glibc is 2.39, as `ubuntu-24.04`'s, and the
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

**Found while building it, older than this plan and on every platform.** Not changed here; the
user is asked.

- While `phpstan-rs.enable` is false, an edit still publishes the file's diagnostics:
  `handle_open_or_change`, the debounced change and the watched files never read `enabled`.
- Disabling clears only the open files. A file the workspace scan published and the editor
  never opened keeps its diagnostics, though the extension's setting says that disabling
  "clears its diagnostics". `published` now records every file the editor holds diagnostics
  for, so the clear could cover them all.

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

### R6. Report paths spelled with `/` where PHPStan spells them so

**Now.** `shown_path` (`project.rs:3430`) gives `src\Foo.php` on Windows. So:

- `ignore_errors::filter` (`:134`) matches `to_string_lossy()` against `src/*`, which never
  matches;
- the baseline writes and matches `normalize_path(..).display()` (`baseline.rs:201`, `:393`).
  A baseline made on Linux does not match on Windows, and the other way round. PHPStan's
  baselines are `/`-joined everywhere (`ParentDirectoryRelativePathHelper`);
- checkstyle and junit (`output.rs:212`, `:244`) write `\`. PHPStan's
  `SimpleRelativePathHelper` writes `/`.

**Change.**

- One function, `report_path(&Path) -> Cow<str>`. On Windows it gives the components joined with
  `/`. On Unix it gives the path's own text, since `\` is a name character there.
- The baseline writes it. On Windows it reads an entry's `\` as `/`, which covers baselines this
  tool wrote there before, and matches it.
- `ignoreErrors`: on Windows a pattern's `\` is a separator, as PHPStan's FNM_NOESCAPE has it
  there. The path and the pattern are compared in `/` form.
- Checkstyle and junit use `report_path`. Text, friendly and json keep the platform's spelling, as
  PHPStan's table and json do.

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

### R7. Case is matched as the root's directory matches it

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

### R8. The include rule's platform values

**Now** (`checker/includes.rs`).

- `PATH_SEPARATOR` is `":"` (`:210`), and `set_include_path` values split on `':'` (`:362`,
  `:377`). On Windows `C:\lib` splits at the drive letter.
- The default include path is Debian's `.:/usr/share/php` (`:47`). Windows PHP's is
  `.;C:\php\pear`.
- `DIRECTORY_SEPARATOR` already takes the platform's (`:214`).

**Change.** The value and the split use the platform's separator: `;` on Windows, `:` elsewhere.
The default is chosen per platform: Unix keeps `.:/usr/share/php` (the directory simply does
not exist on macOS), and Windows gets `.;C:\php\pear`. The constant types in
`predefined_constants.rs`, `'/'|'\\'` and `':'|';'`, stay as they are: those are the types of
code that runs anywhere.

**Test.** `cfg(windows)`: `set_include_path(get_include_path() . PATH_SEPARATOR . __DIR__ .
'/lib')` adds `<dir>/lib`. The Unix tests are unchanged.

**Not here.** Should an include that only the system include path could resolve be reported at
all? That is a rule question on every platform: PHPStan uses its own process's include path. It
goes to the user, and nothing changes here.

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

### R12. Colours on Windows consoles

**Now.** The code frame writes ANSI escapes whenever stdout is a terminal and `NO_COLOR` is unset
(`main/check_project.rs:264-266`, `output/codeframe.rs:16-24`). A Windows console prints them
raw unless virtual-terminal processing is on, and a program must turn that on itself.

**Change.** On Windows, colour is used only once `anstyle_query::windows::enable_ansi_colors()`
returns `Some(true)`. `anstyle-query` 1.1.5 is already in the tree through clap. Other platforms
are unchanged.

**Test.** The decision as a function of (terminal, `NO_COLOR`, enabled).

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

### R17. The records

- `path_bytes`' module doc: which platforms read bytes, and how Windows' grammar differs.
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
- **CRLF semantics** beyond R13's tests:
  - `phpdoc` splits with `str::lines` (CRLF-aware), and its lexer stops at `\r`;
  - the syntax crate handles `\r` at 7 sites.
- **fnmatch's `?`, `[…]` and `\` escapes** in `ignoreErrors` on Unix are a parity gap on every
  platform, not a platform-scoped one.
- **The release target scope** is not changed here: the glibc floor, musl, Windows ARM64.
- **`.cargo/config.toml`'s mold and clang** stay x86_64 Linux's. They are a link-speed choice;
  R15's README covers them.
