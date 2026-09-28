# Crash reporting

phpstan-rs reports its own crashes (panics, stack overflows, aborts) to a
self-hosted Sentry, with a bundle of the inputs that reproduce each one. It is
off unless the user sets a DSN. This plan sits beside `CRATE_SPLIT_PLAN.md`:
it pulls Stage F's single panic hook and internal-error record, and B8's
removal of vendor discovery's hook, forward in their final form (§5), the way
`GATE_SPEED_PLAN.md` pulls items forward.

Status: design. Nothing here is implemented.

## 1. Decisions

- **SDK: `sentry` 0.49.3** (getsentry/sentry-rust), and **`sentry-minidump`
  0.49.3** from the same repository for native crashes (the older
  `sentry-rust-minidump` is deprecated in its favour). A self-hosted server
  works the same as sentry.io; only the DSN differs.
- **Transport: our own outbox transport over `reqwest` + `rustls`** (§4.2).
  Features are additive, so dropping the default `native-tls` (OpenSSL on
  Linux) takes `default-features = false`. `rustls` 0.23 and its `aws-lc-rs`
  provider are already in `Cargo.lock` through the MySQL client, so CI already
  builds them for every target; the release binaries stay free of a
  `libssl.so` runtime dependency, and the aarch64 cross-build needs no arm64
  OpenSSL. `init` installs `aws-lc-rs` as the process's rustls provider
  explicitly, so a second provider entering the graph later can't make rustls
  panic for want of a default. Certificates are checked against the OS trust
  store (`rustls-platform-verifier`), plus the PEM file in
  `PHPSTAN_RS_SENTRY_CA_FILE` when set, for a self-hosted server behind a
  private CA that isn't installed system-wide.
- **Crash events only.** No Sentry logs, no release health, no metrics, no
  performance tracing. Features: `backtrace`, `contexts`, `debug-images`,
  `panic`, `reqwest`, `rustls`. A probe build of exactly this set pulls in no
  `actix-*` crate; the `actix-web` seen in `Cargo.lock` earlier came from the
  default feature set.
- **Opt-in by `PHPSTAN_RS_SENTRY_DSN`, never `SENTRY_DSN`.** `sentry::init`
  falls back to `SENTRY_DSN`, `SENTRY_RELEASE` and `SENTRY_ENVIRONMENT` for any
  option left unset, and a PHP project's CI or container often sets
  `SENTRY_DSN` for the application itself; reading it would send phpstan-rs's
  crashes, with the project's source, to the application's Sentry. phpstan-rs
  reads only its own variable, calls `sentry::init` only when it is set, and
  sets `dsn`, `release` and `environment` explicitly so no fallback applies.
  Unset means no client, no extra process, no network, and no per-file work
  beyond one inactive-hub check. No DSN is compiled into any build.
- **Only the environment can turn it on.** No `phpstan-rs.toml` key sets the
  DSN: a config file travels with a repository, and a cloned repository must
  not be able to send its reader's crashes anywhere. For the same reason the
  VS Code setting is `machine`-scoped (§4.15).
- **Source is sent as-is.** The user opted in, and a report that reproduces
  beats one that protects the code; nothing is anonymised, hashed or scrubbed.
  The README and the VS Code setting's description say exactly what a report
  holds (§4.21).
- **The hostname is not sent.** The contexts integration fills `server_name`
  with it; `before_send` clears it.
- **No report is lost to a process ending.** Every envelope is written to an
  on-disk outbox before it is sent and deleted once the server accepts it
  (§4.12). A native crash or an unisolated panic is finished by a separate
  finisher process (§4.10). A CLI run with a DSN runs under a supervisor that
  waits for those finishers (§4.11), so a CI job that ends right after the
  crash still sends the report.
- **Reduction runs on the developer's side** (§4.18), from the bundle. The
  bundle already holds the full inputs, so reducing on the user's machine
  would add nothing for privacy and would cost the user CPU time.
- **Debug info is kept out of the binaries** and uploaded to Sentry from the
  release workflow (§4.16).
- **Credentials never leave the machine.** The config keys that hold them
  (`ext.sql.reflection`'s `host`, `port`, `user`, `password`, `password_env`,
  `database`, `path`, `session_init_sql`) are dropped from every bundle, crash
  state and recorded command line (§4.8).
- **Two layers, two crates.** Panic isolation (the hook, `isolated`, focus,
  panic records, `entry`, `InternalError`) is a new low crate,
  `crates/crash-context`, with no dependency beyond `backtrace`: the engine
  code that calls it (`project`, and `reflection`'s load steps) moves into
  crates below the app in the crate split (§3.5, §3.10 of
  `CRATE_SPLIT_PLAN.md`), and must not depend on Sentry. Reporting (Sentry,
  the outbox, bundles, the supervisor, the finisher) lives in the app crate's
  `crash_report` module and plugs into the low crate through a `Reporter`
  trait object that `init` registers. Without a registered reporter, the low
  crate still records locations, backtraces and focus for internal errors.
- **A broken pipe isn't a crash.** `phpstan-rs check-project | head` panics
  today (§2); the CLI stops panicking there (§4.20) rather than the reporter
  learning to ignore it.
- **A `crash-reporting` cargo feature**, on by default, carries all of it.
  Without it `init` is a no-op and `sentry`, `reqwest`, the minidump crates
  (with `crash-handler`'s `pthread_create` interposer) and the supervisor's
  signal crates aren't linked, for anyone building a binary with no network
  code. `--no-default-features`, which CI and the gates already build, drops
  it with the rest.

## 2. Today

- **The panic hook is swapped for a silent one** around every isolated batch:
  `main/check.rs:20-33` (`check`), `main/check_project.rs:131-134`
  (`check-project`), `project.rs:1644-1648` (the LSP's
  `check_paths_isolating_panics`) and `composer/src/vendor.rs:64-67` (vendor
  discovery). A panic inside those windows reaches no hook that could report
  it, and those windows are where nearly every caught panic happens.
- **Per-file `catch_unwind`:** `project/session.rs:578` (parse; the panic
  becomes a skipped file, message dropped, counted in "N file(s) skipped: parse
  error"), `project/session.rs:707` (check), `project.rs:1561`
  (property-assign facts; the file contributes nothing, message dropped),
  `project.rs:1673` (LSP check), `main/check.rs:54` (parse-only `check`). A
  caught panic keeps its message at most: no location, no backtrace.
- **The whole-program passes aren't isolated.** `Session::analyse` runs
  `check_whole_program` (dead enum cases, dead functions and classes,
  named-argument facts), `check_mock_hygiene`, `check_class_shapes`,
  `check_banned`, `check_architecture` and `check_composer_hygiene` with no
  `catch_unwind`, inside `check_project.rs`'s silent-hook window. A panic in
  one ends `check-project` with exit code 101 and no message at all: the hook
  that would print it is the silent one, and the restore after `run` is never
  reached.
- **Declaration resolution isn't isolated either.** `Project::finish_loading`
  (`project.rs:703`) runs `canonicalize_class_name_case`, `flatten_traits`,
  `precompute_member_owners`, `resolve_transitive_purity` and
  `precompute_ancestor_names`, most of them as `par_iter` over every class
  (`registry/resolve.rs`, `hierarchy.rs`, `purity.rs`). A panic in one ends
  `check-project` the same silent way. Rayon moves a worker's panic to the
  thread that called into the pool with `resume_unwind`, which runs no hook.
- **`ensure_member_index_built`** (`project.rs:830`), which the dead-enum-case
  rule calls when the main pass didn't seed member references, re-checks every
  file in turn with no isolation, so one file's panic ends the whole pass.
- **The LSP logs a caught panic** as
  `phpstan-rs lsp: checking <path> panicked: <message>` on stderr
  (`lsp.rs:642`).
- **CLI output for a caught panic:** text and friendly formats print
  `<path>: internal error while checking: <message>` and add
  "N file(s) errored while checking" to the summary; JSON and checkstyle drop
  it; the exit code is 0. `check` prints `<path>: <message>` for a panic and a
  parse error alike.
- **LSP, not isolated:** `didOpen`, watched-file events, hover, inlay hints,
  code actions and completion probes run on the main or debounce thread. A
  panic there unwinds holding the `Server` mutex, and the next
  `lock().unwrap()` panics too (Stage F, "Panic isolation, on the pool").
- **LSP configuration:** per workspace folder, `lsp/workspace_config.rs`
  loads `phpstan-rs.toml` from the folder's root or the client's `configPath`
  into a `ConfigFile`; every folder's files go into one `Project`.
- **`process::exit`:** `main.rs:83` (LSP `exit` without `shutdown`),
  `main.rs:91` (`convert-baseline` error), `main/check_project.rs:116,123`
  (config and prepare errors, before any analysis). An exit skips `Drop`,
  which is what flushes a Sentry client.
- **Aborts:** a file with 60 nested arrays overflows the LSP's 2 MiB debounce
  thread on `didChange`; 120 abort it during the startup scan. A 32,000-term
  `.` chain aborts `check-project` (Stage D).
- **Build IDs:** the x86_64 Linux build links with clang + mold
  (`.cargo/config.toml`), and mold writes no GNU build ID unless asked; the
  debug binary at `56c3beab` has none (`readelf -n`). The aarch64 cross-build
  links with Ubuntu's `aarch64-linux-gnu-gcc`, configured with
  `--enable-linker-build-id`.
- **Profiles:** `[profile.release]` has no debug info (`lto = "thin"`,
  `codegen-units = 1`), and `[profile.gate]` inherits it.
- **Release:** five targets (`x86_64`/`aarch64-unknown-linux-gnu` on
  ubuntu-24.04, the aarch64 one cross-compiled; `aarch64`/`x86_64-apple-darwin`;
  `x86_64-pc-windows-msvc`), built with `cargo build --release`, gzipped and
  attached to the GitHub release. The VS Code extension downloads them and
  starts `phpstan-rs lsp` with no environment of its own (`extension.ts:67`);
  its settings `enable`, `serverPath` and `configPath` restart the client when
  changed (`extension.ts:133`). Nothing is on crates.io (`publish = false`).
- **A broken stdout pipe panics.** Output goes through `println!`, which
  panics when stdout is closed: a program printing 200,000 lines into
  `| head -1` exits 101 with "failed printing to stdout: Broken pipe (os error
  32)" (checked with rustc 1.100.0-nightly, 2026-09-21). `check-project`'s
  report, `check`'s summary, `demo` and `convert-baseline` all print this way.
- **A poisoned SQL cache cascades.** `sql::context::SqlContext::reflect`
  (`crates/sql/src/context.rs:61,69`) takes its cache lock with
  `lock().unwrap()`. Its critical sections are a `get` and an `insert`, which
  can't leave the map torn, but a panic while the lock is held would poison it
  and every later file using SQL would fail on the `unwrap`, burying the first
  panic under a group of `PoisonError`s.
- **Processes that run the binary:** `xtask` (gates, corpus, performance with
  `hyperfine`, `xtask/src/main.rs:535,602,762,1033`) and the integration tests
  (`tests/cli.rs`, `lsp_exit_code.rs`, `version.rs`) spawn `phpstan-rs` with
  the developer's environment. `tools/gate-runner` uses the library in-process.
  The fuzz targets call `syntax` directly and never pass through a `catch_unwind`.
- **Terminations no handler sees:** `SIGKILL` (the OOM killer, `kill -9`, a CI
  runner's hard stop) on every OS. On Windows, Rust's `abort` (an allocation
  failure, a panic while panicking) uses `__fastfail`, which ends the process
  without running exception handlers. On Linux and macOS an abort raises
  `SIGABRT`, which `crash-handler` catches.
- **Cache directory:** none exists yet. Stage F2 names the user cache
  directory (`$XDG_CACHE_HOME/phpstan-rs` or the platform equivalent) for the
  result cache.

## 3. What the crates do (read from their source)

- **`sentry-minidump`** spawns the current executable again as a crash-reporter
  process from inside `sentry::init`, with the app's arguments and an
  environment variable marking it. In the reporter, `sentry::init` never
  returns: it runs the minidump server and exits. Code before `sentry::init`
  runs in both processes. The app pings the reporter every 2.5 s; the reporter
  exits after 5 s without one, and when the app disconnects. The spawn uses
  `Command` defaults, so the reporter inherits stdin, stdout and stderr unless
  `on_process` changes them. Breadcrumbs, tags, extras and the user are
  forwarded over its socket; nothing else crosses. On a crash the reporter
  always captures a fatal event with the minidump attached, calling
  `before_capture(scope, path)` first, flushes for up to `flush_timeout`, then
  deletes the dump file (`minidumper-child`'s `on_minidump_created`) and
  exits. A failed upload loses the dump unless the transport keeps it.
- **A Linux minidump carries the process's environment and command line.**
  `minidump-writer` 0.12 copies `/proc/<pid>/environ` and
  `/proc/<pid>/cmdline` into the `LinuxEnviron` and `LinuxCmdLine` streams
  (`src/linux/minidump_writer/mod.rs:422-423`), next to `auxv`, `maps`,
  `status`, `/proc/cpuinfo` and the OS release file. Sent as-is, a native
  crash in CI would upload every secret in the job's environment (cloud keys,
  tokens, the DSN itself, the variable `password_env` names) and any
  `--set` on the command line. Neither `sentry-minidump` nor
  `minidumper-child` offers a way to leave streams out; the reporter's
  `before_capture` can replace the attachment (`Scope::clear_attachments`,
  then `add_attachment`) while the dump file still exists.
- **`crash-handler`** interposes `pthread_create` on Linux and macOS, so every
  thread gets its own alternate signal stack and a stack overflow on a rayon
  worker or the debounce thread is caught. The interposer is linked in whether
  or not a DSN is set; it costs one `mmap` and one `sigaltstack` call per
  thread. The signal handlers are installed only when the integration
  attaches. On Windows a vectored exception handler sees
  `EXCEPTION_STACK_OVERFLOW`; on macOS `SIGABRT` is hooked as well.
- **Hubs are per thread.** The first use of `Hub::current()` on a thread
  other than the main one creates a hub from a snapshot of the main hub's top
  scope (`sentry-core`'s `THREAD_HUB`); breadcrumbs and tags added on the main
  thread afterwards never reach it. A report thread capturing through its own
  hub would lose the LSP's breadcrumbs.
- **`sentry-backtrace`** turns a `backtrace::Backtrace` into a Sentry
  stacktrace (`backtrace_to_stacktrace`) with instruction addresses, which the
  server symbolicates against uploaded debug files.
- **`sentry-panic`** exposes `PanicIntegration::event_from_panic_info`, so a
  hook of our own builds the same event its hook would.
- **Envelopes** serialise with `Envelope::to_writer` and load with
  `Envelope::from_path`; `sentry::transports::RateLimiter` parses
  `X-Sentry-Rate-Limits` and `Retry-After`. A custom transport implements
  `sentry::Transport` and is installed through `ClientOptions::transport`.
- **Defaults:** `shutdown_timeout` 2 s, `max_breadcrumbs` 100, a transport
  queue of 30 envelopes.
- **Dependency policy:** every crate the probe build adds has a license
  `deny.toml` allows (`aws-lc-sys`'s compound expression is already in the
  lock and passes today). `minidump-writer` depends on `nix` 0.30, so the
  supervisor uses `nix` 0.30 too, adding no duplicate.
- **`sentry-cli` 3.8.0** (current on 2026-09-28) has `debug-files upload
  --include-sources`, `debug-files check`, and `releases new`,
  `set-commits`, `finalize`. `sentry-cli-Linux-x86_64` from the 3.8.0 GitHub
  release has SHA-256
  `13f8cb34ae01a6a272d7d7c22e277a105286615b4020de900ea95a8de47cdbb6`; the
  release publishes no checksum file of its own.

## 4. Design

`crates/crash-context` (library name `crash_context`) holds `hook`,
`isolated`, `focus`, `record` (panic records and the deferred list), `entry`,
`internal_error` and the `Reporter` trait. `crates/phpstan-rs/src/crash_report/`
holds `init`, `events`, `bundle`, `state`, `pending`, `supervisor`, `outbox`,
`lsp`, and implements `Reporter`. The `tools/crash-repro` crate uses `bundle`
through the library. Below, `crash_report::isolated` and the like name the
re-exports the app crate keeps for its own call sites.

The hook is installed on first use: `isolated`, `entry` and `focus` each
call `crash_context::install_hook()`, a `Once`, so every user of the library
(the binary, `tools/gate-runner`, the unit tests, `crash-repro`) gets
locations and backtraces in its internal errors without a setup call, and
`main` calls it too, first. The previous hook it keeps is whatever was
installed before (std's, or libtest's capturing one in tests), so test
output capture still works. Functions that are documented as
panic-transparent (`Project::check_paths`, the fuzz targets' entry points)
never pass through `isolated`.

### 4.1 Processes

With a DSN set, a run involves up to five roles of the same binary, told apart
by `_PHPSTAN_RS_CRASH_ROLE`:

| Role | Started by | Does |
| --- | --- | --- |
| supervisor | the user (CLI commands only) | starts the worker, relays its exit status, waits for its finishers (§4.11) |
| worker / app | the supervisor, or the editor for `lsp` | the real run, with the client and the minidump integration |
| reporter | `sentry-minidump`, from the app's `init` | waits for a native crash, turns it into a pending report (§4.9) |
| finisher | the reporter, the panic hook, or a later run's outbox drain | completes a pending report with its bundle and sends it (§4.10) |
| (none) | — | every run without a DSN: no role, no extra process |

`init` decides the role before anything else, in this order: the reporter's
marker `_PHPSTAN_RS_CRASH_REPORTER` (a reporter inherits its app's
environment, role included, so the marker must win); then
`_PHPSTAN_RS_CRASH_ROLE`; then, with neither set, supervisor unless the first
argument is `lsp` (the one command clap hasn't parsed yet that must not be
supervised). Only the app installs the minidump integration; the finisher
gets a client without it; the supervisor builds one only after its worker
ends abnormally (§4.11).

Every child the plan starts (worker, finisher) gets its stdin and stdout set
to null except the worker, which inherits the supervisor's. A detached
finisher's stderr goes to `finisher.log` in its pending entry, since the
stderr it would inherit (an editor's log pipe, a terminal) can close before
it finishes. Detached finishers run in their own process group
(`CommandExt::process_group(0)` on Unix, `DETACHED_PROCESS |
CREATE_NEW_PROCESS_GROUP` on Windows), so a Ctrl-C meant for the run doesn't
stop a report already on its way.

`init` failing leaves the run without reporting, with one line on stderr: an
invalid DSN, or an unreadable `PHPSTAN_RS_SENTRY_CA_FILE` (then only the
system roots are trusted).

A process that can't be started degrades instead of failing the run. The
supervisor failing to start its worker (`current_exe` unavailable, as when the
binary was replaced during an upgrade and `/proc/self/exe` names a deleted
file) runs the command itself as the app, unsupervised, with one line on
stderr. `sentry-minidump` failing to start its reporter leaves the app without
native-crash reports (it logs through `sentry_debug!`, which we surface as one
stderr line). A finisher that can't be started leaves its pending entry for
the next drain.

### 4.2 Client and transport

`main` starts with `crash_report::init()`, before `Cli::parse`, the dhat
profiler and anything else, because the reporter re-executes the binary and
must diverge before any app work. With `PHPSTAN_RS_SENTRY_DSN` unset it
returns `None` and nothing else in this plan runs. With it set:

- `dsn` from the variable; `release` = `phpstan-rs@<version>+<commit>`, the
  commit from `PHPSTAN_RS_BUILD_COMMIT` at compile time (`option_env!`, set by
  the release workflow), `+dev` when absent. A build script reading
  `git rev-parse HEAD` would give development builds their commit, but its
  output changes on every commit and would force the crate, and everything
  above it, to recompile each time; instead every event carries the binary's
  identity, its build ID (the debug-images integration's `code_id`) and its
  executable's size and modification time as the extra `binary`, which tell
  two development builds apart; `environment` = `release` for a
  `dist` build (`PHPSTAN_RS_DIST` at compile time), `development` otherwise;
- `default_integrations = false`, then the backtrace, contexts and
  debug-images integrations, and the minidump integration in the app role; the
  panic integration's own hook is not installed, because the single hook
  (§4.3) calls it;
- `send_default_pii = false`; `before_send` clears `server_name`;
- `in_app_include`: the workspace crates (`phpstan_rs`, `syntax`, `types`,
  `phpdoc`, `sql`, `diagnostics`, `composer`, `config`, `stub_sources`), so
  Sentry folds std, rayon and the SDK out of the trace;
- `shutdown_timeout` 30 s for CLI commands, so a report usually arrives
  within the run that made it (a CI runner's disk, and so its outbox, is gone
  after the job); before waiting, the run prints
  `phpstan-rs: sending N crash report(s) to <host>` to stderr. 2 s for `lsp`,
  whose editor is waiting for it to exit: whatever isn't sent by then stays in
  the outbox for the next start;
- `transport`: the outbox transport. For an envelope holding an event, it
  writes the envelope to the outbox (§4.12), synchronously inside
  `send_envelope`, then queues it for a sender thread; SDK-internal envelopes
  (client reports) are sent without being kept. The sender POSTs with its own
  `reqwest` client (the platform verifier plus `PHPSTAN_RS_SENTRY_CA_FILE`,
  `HTTPS_PROXY`/`NO_PROXY` honoured as `reqwest` does by default, a 10 s
  connect timeout and a 120 s request timeout, enough for 10 MiB on a slow
  link), applies `RateLimiter`, and on a 2xx deletes the entry and, when the
  event carries a `bundle_key` extra, records that key in `sent.json` (§4.6),
  so a bundle counts as sent only once the server has it. A network error or
  a 5xx keeps the entry for a later run; a 429 keeps it with `not_before` set
  from the limit; any other 4xx deletes it with a line on stderr, since
  resending can't succeed. When the outbox can't be written (a full or
  read-only disk), the envelope is sent directly, unkept, with one line on
  stderr.
- A pending event (tagged `bundle = pending`, §4.6) is stored in
  `pending/<entry id>/`, the id taken from the event's `pending_entry` extra,
  and the transport then starts its finisher detached (§4.10). The code that
  tags an event pending (the hook, the reporter's `before_capture`) chooses
  the id and writes the crash-state copy and paths into that directory first,
  since the event id doesn't exist until the capture.

The guard lives in `main` for the whole run. Every `process::exit` goes
through `crash_report::exit(code)`, which flushes first: `main.rs:83`,
`main.rs:91` and `main/check_project.rs:116,123`.

A user cache directory, `crash_report::cache_dir()` (`dirs::cache_dir()` +
`phpstan-rs`), holds the outbox, the crash state and `sent.json`, and is
created with mode `0700` on Unix, since it holds source. When there is no
user cache directory (no `HOME` in a container) or it can't be written, it
falls back to `std::env::temp_dir()/phpstan-rs-<uid>` (the user's name on
Windows), created `0700` and refused if it exists owned by someone else; when
that fails too, the run keeps nothing on disk: envelopes are sent directly
(§4.2's unwritable-outbox path), and a native crash is reported without
crash state, so without a bundle. An internal `_PHPSTAN_RS_CACHE_DIR`
overrides the location, for tests running in parallel. Stage F2's result
cache reuses the function (§5).

All crash-report code goes through `Hub::main()`, never `Hub::current()`
(§3): breadcrumbs, tags and context are set on it, and events from the report
thread, the rayon workers and the hook are captured through it, so every event
carries the whole run's breadcrumbs. `crash_report::breadcrumb`, `tag` and
`extra` also forward to the minidump integration
(`sentry::with_integration`), which carries nothing across on its own.

In a `hotpath` build, the `#[hotpath::main]` wrapper runs before `init`, so a
reporter or finisher starts a hotpath guard it never reports from (it exits
through `process::exit`); profiling builds are never run with a DSN.

### 4.3 Panic hook

One hook, installed once in `main` after `init`, replaces all four swaps in
§2. Three thread-locals drive it: `isolated` (inside an isolated unit),
`focus` (§4.7) and `reporting` (inside crash-report code, §4.13).
`crash_report::isolated(unit, || ...)` sets and clears `isolated` around a
`catch_unwind`; every site in §2 and every whole-program pass (§4.5) runs
through it. The hook:

- **when `reporting` is set** prints the default message and does nothing
  else;
- **on an isolated thread** prints nothing and stores a panic record (the
  message, location, the thread's focus at that moment, and an unresolved
  `backtrace::Backtrace`) in a thread-local, which `isolated` takes into the
  unit's internal error (§4.4). The backtrace is captured when a client is
  active or `RUST_BACKTRACE` is set, and resolved only when printed or sent;
- **on a worker-pool thread outside an isolated unit** prints nothing and
  stores the record in a process-wide deferred list, newest first, holding at
  most 64. Rayon carries such a panic to the thread that called into the pool,
  with `resume_unwind`, which runs no hook, so the decision belongs to
  whichever catch receives it: an `isolated` unit up the calling thread (a
  load step running `par_iter`, §4.5) takes the newest record whose message
  equals its payload's, and so records the location and backtrace from the
  worker, not from the resume. Records nobody takes age out of the list;
- **anywhere else** prints the default message (the previous hook, kept) and,
  with a client, builds the event with `PanicIntegration::event_from_panic_info`
  and captures it as a pending report (§4.6). A location already reported by
  this process is counted, not reported again, and a process makes at most 3
  pending reports: a panic that poisons the LSP's `Server` mutex is followed by
  a `PoisonError` panic on every later lock, and those would otherwise each
  start a finisher.

The hook itself never panics: every step returns a `Result` that it ignores,
and it runs no allocation-heavy work beyond building the event.

A deferred panic that no isolated unit catches reaches the top of a thread.
`crash_report::entry(|| ...)` wraps `main`'s body and every thread the program
spawns (the LSP's reader, debounce and report threads). When a panic escapes,
it takes the matching deferred record, prints the default message the hook
held back, reports it as a pending report (§4.6) when there is a client, and
resumes unwinding, so the process or thread ends as it does today.

Vendor discovery's swap is deleted rather than converted: its parse already
returns errors (B8, "No global panic hook"). The `composer` crate gains no
dependency.

### 4.4 Internal errors and their output

`isolated` turns a caught panic into an internal error:

```rust
pub struct InternalError {
    pub unit: Unit,              // Parse(path) | Facts(path) | Check(path) | Pass(name) | Load(step)
    pub focus: Option<PathBuf>,  // the panicking thread's focus (§4.7)
    pub message: String,
    pub location: Option<Location>, // file:line:col in phpstan-rs's source
    pub backtrace: Option<backtrace::Backtrace>,
}
```

The parse site stops dropping the panic into a skipped file, and the facts
site stops dropping it into an empty contribution: both record an internal
error. `AnalysisResult::check_panics` becomes `internal_errors:
Vec<InternalError>`, sorted by unit.

Output, a behaviour commit with its `tests/cli.rs` changes:

- text and friendly print one line per internal error,
  `<path>: internal error while parsing|collecting property facts|checking:
  <message> (at <location>)`, or for a pass
  `internal error in <pass>: <message> (at <location>)` followed by
  `(while on <focus>)` when set; with `RUST_BACKTRACE` set, the resolved
  backtrace follows, indented;
- the summary says "N internal error(s)" in place of "N file(s) errored while
  checking", and a parse panic no longer counts in "skipped: parse error";
- `check` prints `<path>: internal error: <message> (at <location>)` for a
  panic, keeping `<path>: <message>` for a parse error;
- a pass that panics contributes no diagnostics; the other passes still run;
- a load step that panics ends the run: `Session::run` returns the internal
  error, printed as `internal error while <step>: <message> (at <location>)`
  plus the focus, the way it prints its other errors today;
- the LSP's log line becomes
  `phpstan-rs lsp: internal error while checking <path>: <message> (at
  <location>)`.

JSON, checkstyle and JUnit output and the exit code stay as they are until
Stage F's "Internal errors are reported in every format" and "The exit code".
The JSON's `totals.files_checked` keeps excluding every file with an internal
error, now including the parse panics it used to count as skipped, so its
value doesn't change.

### 4.5 Whole-program passes

`Session::analyse` runs each pass (`dead_enum_cases`, `dead_code`,
`named_arguments`, `mock_hygiene`, `class_shapes`, `banned`, `architecture`,
`composer_hygiene` per root) as its own `isolated(Unit::Pass(name), ..)`.
`check_whole_program`'s three rule families become three passes, so a panic
in one keeps the others' results. Each pass sets the focus (§4.7) as it moves
between declarations, so its internal error, and its bundle, name the file it
was on.

`ensure_member_index_built` runs each file's re-check as its own
`isolated(Unit::Check(path), ..)`: a file that panics contributes no member
references, and the pass goes on with the rest.

`finish_loading`'s five steps each run as `isolated(Unit::Load(step), ..)`
(`canonicalize_class_names`, `flatten_traits`, `member_owners`,
`transitive_purity`, `ancestor_names`). Their `par_iter` closures set the
focus to the declaring file of the class they're on, so a panic on a worker
carries it in its deferred record. A failed step leaves the registry
unusable, so the run ends with that internal error rather than analysing
against half-resolved declarations. The LSP's incremental resolution
(`project.rs:987`, on the debounce thread) stays unisolated until Stage F
moves it onto the pool; its panics are reported through `entry` (§4.3).

### 4.6 From crashes to events

- **Messages:** an event's message is the panic message cut at 8 KiB on a
  character boundary (a `format!`ed panic can embed a whole AST or type);
  the full message goes into the bundle's manifest. The CLI's own line keeps
  the whole message.
- **Isolated panics, CLI:** after analysis and before the report, internal
  errors are grouped by panic location. Each group becomes one event: message
  and stack trace from the group's first error, fingerprint
  `["panic", <location>]` (stable across symbolicated and unsymbolicated
  builds), tags and context (§4.7), the group's file count and first 20 paths
  as extras (Sentry trims large extras; the full list goes in the bundle's
  manifest), and a bundle (§4.8) for the group's smallest file, the cheapest
  to reproduce.
- **Isolated panics, LSP:** a `phpstan-rs-crash-report` thread takes internal
  errors over a channel. The thread holding the `Server` lock computes the
  bundle's file list and copies the overlay text it includes (lookups only),
  then hands off; reading, archiving and sending happen off the lock. Within a
  session, a location already reported is counted, not sent again.
- **Unisolated panics and native crashes** become pending reports: the code
  that catches the crash creates `<cache>/crash-outbox/pending/<entry id>/`
  with a copy of the crash state (§4.9) and the focus or in-progress paths,
  then captures the event tagged `bundle = pending` with the id as the extra
  `pending_entry`; the outbox transport stores it there instead of sending it
  and starts the finisher (§4.2, §4.10).
- **Across runs:** `<cache>/crash-reports/sent.json` records a bundle key,
  `(release, panic location, crashing file's content hash)` for a panic and
  `(release, signal or exception code, in-progress files' content hashes)` for a
  native crash, for each bundle the server accepted (§4.2). A repeat within 30
  days still sends the event, so Sentry's counts stay true, but tagged
  `bundle = already_sent` and without the bundle. The file is updated under `File::lock`
  (std) and replaced by rename, so concurrent runs don't lose entries.

### 4.7 Tags, context, breadcrumbs and focus

- **Tags:** `command` (`check`, `check-project`, `lsp`), `level`, `unit`
  (`parse`, `facts`, `check`, `pass:<name>`, `load:<step>`, `native`), `pcre_validate`,
  `lsp.client` and `lsp.client_version` (from `initialize`'s `clientInfo`),
  `bundle` (`attached`, `already_sent`, `too_large`, `no_focus`, `failed`,
  `pending`).
- **Context `run`:** analysed and context file counts, the enabled optional
  rules, the PHP version range, whether ext-sql reflects a live database.
- **Breadcrumbs:** each run phase (config, discovery, load, property-assign
  facts, check, each whole-program pass, report); in the LSP each request and
  notification's method, file and position, never its text. The SDK keeps the
  last 100; `crash_report::breadcrumb` also sends each one to the reporter
  (§4.2).
- **Focus:** `crash_report::focus(path)` sets the thread's current file. The
  per-file units set it from their path; whole-program passes as they move
  between declarations (a class's or function's declaring file, a composer
  manifest); the LSP dispatch from the request's document, and the debounce
  worker from the file it rechecks. In the app role each change is also sent
  to the reporter as the extra `focus.<thread name>`; the worker pool's
  threads are named `phpstan-rs-worker-<n>` for this (`ThreadPoolBuilder::
  thread_name`). `isolated` clears it when its unit ends, and the reporter's
  extra with it, so an idle thread never names a file it has finished.
- **Two levels of focus.** A unit's focus (the file of a per-file unit, the
  current pass or load step) is forwarded to the reporter; a declaration's
  focus inside a pass or load step (one per class, tens of thousands in a
  large project) stays in the thread-local, where the in-process record
  reads it. Setting either costs a thread-local write; a unit's also costs one
  socket message when a reporter is attached, so a native crash names the file
  a unit was on and a panic names the declaration too. Step 8 measures
  `check-project` over the Piwigo corpus with and without a DSN; the overhead
  must stay under 2 % of wall time, or the forwarding is batched per worker.

### 4.8 Reproduction bundle

One `repro.tar.gz` attachment per event, built by one function
(`bundle::build`) for every kind of crash:

```text
manifest.json
phpstan-rs.toml          the effective config, rewritten (below)
project/<path>           files at their paths under the project root
external/<n>/<name>      files outside the root, mapped in the manifest
```

- **`manifest.json`:** the release and commit, the command line, the crash
  (unit, message, location or signal, focus), each file with its role
  (`crashing`, `dependency`, `dependent`, `stub`, `composer`), size, SHA-256
  and whether it came from an unsaved editor buffer; files left out for the
  cap, with their roles and sizes; for the LSP, the workspace folders.
- **Files, per unit:**
  - `parse`, and `check`'s parse-only run: the file alone;
  - `facts` and `check`: the file, then breadth-first by reference distance
    the files declaring what it references (each name in its
    `FileState::references`, through the registry's `Provenance::file`), a
    class's parents, interfaces and traits always one level ahead of anything
    else at the same distance;
  - `pass:<name>`: the focus file, then the same walk run both ways, through
    references and through `Project::dependents` (the files referencing a
    declaration are what whole-program passes read); with no focus, the
    manifest alone, tagged `no_focus`;
  - `load:<step>`: as `pass:<name>`, from the focus;
  - `native`: every in-progress file (the dump alone doesn't say which thread
    caused it), each walked as `check`.

  Then the user's stub files (`stub_paths`) and `composer.json` and
  `composer.lock` from each discovered root (they set the PHP version range).
  The walk fills the bundle up to the cap rather than stopping at a depth.
- **Cap:** 10 MiB compressed or 64 MiB uncompressed, whichever comes first.
  Self-hosted Relay accepts envelopes and attachments up to 100 MiB by default,
  but a CLI run waits for the upload; 10 MiB of gzipped PHP is roughly 60 MiB
  of source. A crashing file larger than the cap alone is sent as the manifest
  only, tagged `too_large`.
- **Config:** the effective `ConfigFile` table (old keys moved, `--set`
  applied) serialised back to TOML. `config::Schema` gains two key
  registries, filled by the sections that own the keys:
  - **path keys**, rewritten into the bundle: `paths` becomes the crashing
    files; `scan_paths` becomes the dependency and dependent files; `stub_paths`
    points at the bundled stubs; `ext.sql.reflection.schema_file` points at the
    bundled DDL file; `baseline` and `ignore_errors` are dropped, since they
    act after the check;
  - **secret keys**, dropped everywhere: `ext.sql.reflection`'s `host`,
    `port`, `user`, `password`, `password_env`, `database`, `path` (a SQLite
    database file) and `session_init_sql`. A `--set` on a secret key is shown
    as `--set <key>=<redacted>` in the recorded command line.

  A schema test fails when a section deserialises a `PathBuf` field whose
  key is in neither registry, so a new path key can't be missed. In the LSP,
  the folder's config as `workspace_config::resolve` loaded it.
- **Live SQL reflection:** when ext-sql reflected a live database, the
  reflected catalog is written as DDL (`SchemaCatalog::to_ddl`, new, the
  inverse of `parse_ddl` for the tables, columns and types the catalog holds)
  into the bundle as `live-schema.sql`, and the bundle's config reads it as
  its `schema_file`. What a live connection adds beyond the catalog (whether
  the server accepts a query at prepare time) isn't in the bundle; the
  manifest says `sql: live` so a replay that differs there is explained.
- **Contents as analysed:** a file comes from the text the project holds
  when it holds it (an LSP overlay, or a source it retained); otherwise from
  disk, and a file whose modification time is later than the run's start is
  marked `changed_since_crash` in the manifest, since the bundle may not hold
  what crashed.
- **Paths:** stored in the archive with `/` separators on every platform. A
  path that isn't UTF-8 is exact in the tar header and lossy, with a flag, in
  the manifest.
- **Archive:** `tar` + `flate2` (`flate2` is already in the lock).

### 4.9 Crash state and minidumps

- **Crash state:** the directory `<cache>/crash-state/<run id>/`, created by the
  supervisor when there is one (so it knows the path) and by the app otherwise,
  passed down in `_PHPSTAN_RS_CRASH_STATE`. The app writes `state.json`: a
  format version, the role, command line, working directory, project root,
  config path and effective config table, secret keys dropped (§4.8); in the LSP
  the workspace folders, each folder's config path and table, the client's
  `initializationOptions` and `PhpstanRsSettings`, and `clientInfo`. The app
  holds `app.lock` in it for its whole life (`File::lock`), and the reporter
  holds `reporter.lock` for its own; a directory whose `app.lock` can be taken
  belongs to no live process. The LSP writes an overlay into `overlays/` when
  `didOpen` brings text that differs from the file on disk (an editor restoring
  an unsaved buffer) and whenever the debounce worker takes a change, and
  removes it on `didSave` and `didClose`. The app deletes the directory on a
  normal exit; the outbox drain (§4.12) removes directories whose `app.lock` and
  `reporter.lock` are both free.
- **Integration:** `MinidumpIntegration` with `server_env_var`
  `_PHPSTAN_RS_CRASH_REPORTER`, `process_name` `phpstan-rs crash reporter`,
  `crashes_dir` `<cache>/crashes`, and `on_process` nulling the reporter's
  stdin and stdout (under `lsp` they are the protocol pipes; stderr stays).
- **Scrubbing the dump:** `before_capture` first reads the dump file,
  blanks the data of its `LinuxEnviron` (`0x47670007`) and `LinuxCmdLine`
  (`0x47670006`) streams in place (the header's stream count and directory
  offset, then each 12-byte directory entry's type, size and offset: no parser
  crate needed), and replaces the attachment with the scrubbed bytes. Stream
  types other platforms' writers don't produce are simply absent. What a dump
  still holds is each thread's stack memory, which can include fragments of
  whatever the program was handling (source text, and during a live SQL
  connection its credentials); the README says so (§4.21), and the tests
  check that a canary environment variable and a canary `--set` value never
  appear in an uploaded dump, on Linux, macOS and Windows.
- **In the reporter's `before_capture`:** it creates the pending entry,
  copies the crash state and the current `focus.*` paths into it, and tags the
  event `bundle = pending` and `unit = native` with the entry's id; the outbox
  transport then stores the fatal event, minidump included, in that entry and
  starts the finisher (§4.2). The reporter's own flush finds nothing left to
  send, and it exits, releasing `reporter.lock`.

### 4.10 Finisher

`phpstan-rs __crash-finish <pending entry>`, a subcommand hidden from
`--help` (`#[command(hide = true)]`):

1. takes the entry's lock with `File::try_lock`, and exits if another
   finisher holds it;
2. increments the entry's attempt count on disk before anything that could
   crash;
3. checks `sent.json` (§4.6): a report whose bundle was already sent within
   30 days goes out without one, tagged `already_sent`, skipping 4;
4. on the first two attempts, and when the crash state's format version is
   the finisher's own, loads the project from the entry's crash state
   (parse and index only, no checking: `Session` from `state.json` for
   `check-project`; for `lsp`, `lsp::load_workspace(folders, settings,
   overlays) -> Project`, the load `Server::add_workspace_folders` does today
   moved into a function the server and the finisher both call, with no
   transport; nothing for `check`, whose bundle is the file alone) and builds
   the bundle (§4.8);
5. on a third attempt, or when the load or build fails, attaches the crashing
   files and the effective config directly, tagged `bundle = failed`, with the
   failure as an extra;
6. adds the attachment to the stored envelope, sets the `bundle` tag, moves
   the entry into the outbox proper, and sends it.

A finisher that crashes while loading leaves the entry for the next attempt,
so a project that crashes the loader can't loop forever. Its own client has
no minidump integration and records nothing about its own crashes.

### 4.11 Supervisor

Every command but `lsp` runs under a supervisor when a DSN is set. The process
the user started re-executes itself (`std::env::current_exe`) as the worker with
the same arguments (`std::env::args_os`, so non-UTF-8 arguments pass through
exactly), inherited stdio, and a run id in the environment. It waits for the
worker, then:

- on a normal exit, exits with the worker's code;
- after an abnormal end that left no pending report for the run (on Windows,
  a `__fastfail` abort, exit code `0xC0000409`; anywhere, a crash the
  reporter missed), creates one itself: a fatal event
  "worker terminated by `<signal or exception code>`" with `unit = native` and
  the run's crash state, finished like any other (§4.10), with no focus, so
  its bundle is the manifest and config alone. Terminations from outside
  (`SIGKILL`, `SIGTERM`, `SIGINT`, `SIGHUP`) are not reported: they are a
  user's, a CI runner's or the OOM killer's decision, and nothing tells them
  apart. For this the supervisor builds a client after the worker ends, only
  when it needs one;
- whatever the exit, checks for the run's reports: it waits until
  `reporter.lock` in the run's crash state can be taken (the reporter has
  written its pending report and gone, or never had one) and until no pending
  entry carrying the run id is unfinished or locked by a finisher, for up to
  60 s in all, printing `phpstan-rs: sending the crash report to <host>` once
  it starts waiting on one. With no crash this takes one lock attempt and one
  directory listing;
- then exits with the worker's code, or after a crash with 128 + the signal
  number on Unix and the worker's exception code on Windows.

The worker must not outlive the supervisor. On Unix the supervisor forwards
`SIGTERM`, `SIGHUP` and `SIGINT` to it (`signal-hook`'s iterator and
`nix::sys::signal::kill`, no `unsafe`), so a CI timeout still stops the
analysis, and on Linux the worker also sets `PR_SET_PDEATHSIG` to `SIGTERM`
(`nix::sys::prctl::set_pdeathsig`) against a supervisor killed outright,
then checks that its parent is still the supervisor (`getppid` against the
pid the supervisor passes down), and exits if not, since the supervisor may
have died before the call. On
Windows the worker is started in a job object with kill-on-close
(`win32job`), so ending the supervisor ends the worker. macOS has no
parent-death signal; a supervisor killed with `SIGKILL` leaves its worker to
finish its run. A `SIGINT` while the supervisor waits for reports ends the
wait at once; the finishers are in their own process group and
carry on. The LSP runs without a supervisor: the
editor restarts it, and its detached finishers run on the same machine.

### 4.12 Outbox

`<cache>/crash-outbox/` holds `ready/` (envelopes waiting to be sent) and
`pending/` (reports waiting for their finisher). Each entry is a directory:

```text
envelope        the serialised envelope
meta.json       format version, DSN hash, created, attempts, not_before, run id, bundle key
entry.lock      empty; held by whoever is writing, sending or finishing it
state/          pending entries only: the crash state copy and paths
finisher.log    pending entries only
```

Every lock in this plan is a dedicated empty `*.lock` file, never the data it
guards: Windows locks are mandatory, so locking `envelope` itself would stop
another process from reading it. A sender takes `entry.lock` with `try_lock`
before sending and skips the entry when it can't, so two runs draining at once
never send one report twice; the transport holds it from writing the entry until
it is sent or left for later. An entry is sent only by a client with the same
DSN hash, so switching servers never sends old reports to the new one. An entry
whose `meta.json` format version is newer than the running binary's is left
alone (a downgrade); an older one is read by the version's own rules, which each
format change keeps. At start, the app and the LSP drain it on a background
thread: `ready/` entries are sent, `pending/` entries whose lock is free are
handed to a finisher, stale crash-state directories are removed, and dumps older
than a day in `<cache>/crashes` (left by a reporter killed mid-upload) are
deleted. Entries older than 30 days are deleted, which is also as far back as
Relay accepts an event's timestamp by default (`max_secs_in_past`), and the
oldest go first once the outbox passes 100 MiB.

### 4.13 Failures in the reporting itself

- Everything crash-report code does on the app's threads runs with
  `reporting` set and under `catch_unwind`. A panic there is printed and
  isolated; the event goes out without its bundle, tagged `bundle = failed`,
  with the panic message as the extra `bundle_error`.
- A stack overflow while building a bundle in-process is a native crash of the
  app, which the reporter handles like any other.
- The server unreachable: the envelope stays in the outbox. A CLI run that
  can't send within `shutdown_timeout` prints
  `phpstan-rs: N crash report(s) kept in <outbox> for the next run`.
- A CLI run interrupted (Ctrl-C, `SIGTERM`) after an isolated panic but
  before the end of the run loses its event, since grouping and bundling
  happen at the end. The run's own report, printed at the same point, is lost
  the same way, so the two stay consistent; a crash that recurs is reported
  by the next run that finishes.
- A rejected envelope (4xx other than 429): deleted, with
  `phpstan-rs: the crash-report server rejected a report (<status>)` on
  stderr.

### 4.14 LSP specifics

- **Focus and breadcrumbs:** `run_dispatch_loop` sets the focus and adds a
  breadcrumb per message before `handle_request`, `handle_notification` or
  `queue_change`; the debounce worker does the same per recheck.
- **Isolated panics** (the workspace check through
  `check_paths_isolating_panics`) go to the report thread (§4.6).
- **Unisolated panics** in a handler are pending reports finished by a
  detached finisher (§4.6, §4.10); the LSP's behaviour after the panic is
  unchanged (Stage F moves those handlers onto the pool).
- **Client:** `initialize` sets the `lsp.client` tags and writes `clientInfo`
  into the crash state.
- **Shutdown:** on `exit`, the dispatch loop closes the report thread's
  channel and waits up to 2 s for it to hand what it holds to the transport;
  anything unfinished is already in the outbox.

### 4.15 VS Code

- A setting `phpstan-rs.crashReporting.dsn` (string, empty by default,
  `"scope": "machine"`, so only user settings on that machine can set it: no
  workspace's `.vscode/settings.json` can, and Settings Sync doesn't carry it).
  Its description says what reports hold.
- `serverOptions.options.env` passes it as `PHPSTAN_RS_SENTRY_DSN` when set,
  over `process.env`.
- The `onDidChangeConfiguration` handler (`extension.ts:133`) restarts the
  client when it changes, as it does for `serverPath`.

### 4.16 Debug files and the release workflow

- **`[profile.dist]`** inherits `release` with `debug = "limited"` (functions
  and line tables; symbolication uses nothing more). `release` and `gate` are
  unchanged.
- **Build IDs:** `.cargo/config.toml` adds `-C link-arg=-Wl,--build-id` to
  `x86_64-unknown-linux-gnu`'s rustflags and gives
  `aarch64-unknown-linux-gnu` the same flag.
- **Build jobs** build with `--profile dist`, `PHPSTAN_RS_DIST=1` and
  `PHPSTAN_RS_BUILD_COMMIT=${{ github.sha }}`; the packaging and smoke-check
  steps read `target/<target>/dist/` in place of `target/<target>/release/`.
  Then they split per target:
  - Linux: `objcopy --only-keep-debug` into `phpstan-rs.debug`, then
    `objcopy --strip-debug --add-gnu-debuglink` on the binary
    (`aarch64-linux-gnu-objcopy` on the cross leg, from the binutils it
    installs already);
  - macOS: `CARGO_PROFILE_DIST_SPLIT_DEBUGINFO=packed` writes a `.dSYM`, and
    `strip -S` strips the binary;
  - Windows: MSVC writes the `.pdb` beside the executable.

  Each job uploads the stripped binary and its debug file as a workflow
  artifact (`actions/upload-artifact`, pinned by commit SHA like the
  workflow's other actions), and packages the stripped binary as today.
- **An `upload-debug-files` job** on ubuntu-24.04, after every build job,
  checks out the repository with full history (`fetch-depth: 0`), downloads
  the build jobs' artifacts (`actions/download-artifact`, pinned), downloads
  `sentry-cli-Linux-x86_64` 3.8.0 and checks its SHA-256 (§3), then:
  - `sentry-cli debug-files check` on every binary, failing the job when one
    has no debug ID, so a missing build ID is caught at release time;
  - `sentry-cli debug-files upload --include-sources --wait` on every binary
    and debug file (the binaries carry the unwind tables the server also
    reads);
  - on a tag, `releases new`, `releases set-commits --local` and
    `releases finalize` for `phpstan-rs@<version>+<sha>`. `--local` reads the
    checked-out history; `--auto` would need a repository integration
    configured in Sentry, which a self-hosted instance may not have.

  Its secrets are `SENTRY_URL`, `SENTRY_ORG`, `SENTRY_PROJECT` and
  `SENTRY_AUTH_TOKEN`, an organization auth token. A `workflow_dispatch` run
  uploads debug files but creates no release.

### 4.17 Server setup

Recorded in `docs/crash-reporting-server.md` and done once on the self-hosted
instance:

- a project `phpstan-rs`, platform Rust, with its own client key; the key's
  rate limit set to 60 events a minute, so a crash on every file of a large
  run can't flood the server (the client's grouping (§4.6) already sends one
  event per location per run);
- **Security & Privacy:** "Store Native Crash Reports" on, so minidumps stay
  downloadable; the data scrubber and default scrubbers left on. They match
  sensitive key names and value patterns, and none of our keys match; they
  don't read attachments. "Prevent storing of IP addresses" on;
- **attachments access** limited to the maintainers' role;
- **grouping:** the default, plus the client's `["panic", location]`
  fingerprint for panics; native crashes group by their symbolicated stacks;
- **alerts:** a new issue and a regressed issue each notify by email;
- **retention:** the instance's `SENTRY_EVENT_RETENTION_DAYS` (90 by
  default) also bounds how long bundles are kept;
- **an organization auth token** for the release workflow, stored as
  `SENTRY_AUTH_TOKEN`.

### 4.18 Developer tools

`tools/crash-repro`, a workspace member depending on the `phpstan-rs` library,
so the user's binary doesn't carry it:

- **`replay <bundle>`** extracts the bundle into a temporary directory and
  runs it with the bundle's config and command, reporting whether the same
  panic location fires, or, for a native crash, whether a child running it
  dies of the same signal.
- **`reduce <bundle>`** shrinks the bundle while it still reproduces: it
  deletes AST nodes by span (statements, class members, function bodies,
  arguments, operands) across every file, keeping a deletion only if the
  crash still happens, then drops files nothing references any more. Panics
  are checked in-process under `catch_unwind`. Native crashes are checked in a
  worker process that writes its candidate to a state file before each
  attempt; when the worker dies of the same signal, the supervisor records
  the candidate as reproducing and starts a new worker from the saved state.
  The result is a new bundle and a fixture directory ready to commit.
- **Development builds' symbols.** An isolated panic's backtrace is
  resolved in-process before sending, so a development build, which keeps its
  debug info, sends frames with function, file and line. Its minidumps are
  symbolicated on the server, which has no debug files for a local build:
  `cargo xtask crash-debug-files [debug|release]` uploads the local binary
  with `sentry-cli debug-files upload --include-sources`, using the same
  `SENTRY_URL`, `SENTRY_ORG`, `SENTRY_PROJECT` and `SENTRY_AUTH_TOKEN` as
  `fetch`, for a maintainer running development builds with a DSN.
- **`fetch <issue>`** downloads an issue's latest bundle through the Sentry
  API with `SENTRY_URL`, `SENTRY_ORG`, `SENTRY_PROJECT` and a
  `SENTRY_AUTH_TOKEN` with `event:read`: `GET /api/0/issues/<id>/events/latest/`
  for the event id, then the event's attachments list
  (`/api/0/projects/<org>/<project>/events/<event>/attachments/`) and the
  `repro.tar.gz` entry with `?download=1`.
- **Bundles are untrusted input**, since anyone with the DSN can send one.
  Extraction rejects absolute paths, `..` components, links and device
  entries, and stops past 256 MiB extracted; a bundle is only ever extracted
  into a fresh temporary directory, and replay never runs anything but
  phpstan-rs on it.

### 4.19 Tests

- **Unit:** the hook's four paths, with the `sentry` `test` feature capturing
  events in memory; `isolated` recording location, backtrace and focus; a
  panic inside a `par_iter` under an isolated unit recorded once, with the
  worker's location and focus; `entry` printing and reporting a deferred
  panic that escapes; the bundle
  builder (the walk's order, ancestors first, both directions for passes, the
  cap and `omitted`, the config rewrite, overlays); grouping; `sent.json`
  under two concurrent writers, and written only after a 2xx; the outbox
  transport's status handling, DSN matching, `not_before`, and two senders
  draining one outbox sending each entry once; the unwritable-outbox fallback;
  the finisher's attempt counting.
- **End to end:** a test Sentry server on `127.0.0.1` decodes envelopes,
  served over TLS with a certificate from a test CA that
  `PHPSTAN_RS_SENTRY_CA_FILE` points at, so every test also exercises rustls
  and the CA option. A `crash-injection` cargo feature, off by default, makes
  the binary panic, overflow its stack or panic outside isolation on a named
  file or in a named pass. `tests/cli.rs` covers:
  - a check panic: one event, its tags, the bundle's files and config;
  - a pass panic: the other passes' diagnostics still reported, the bundle
    walked from the focus;
  - a stack overflow: the supervisor's exit status, a fatal event with the
    minidump and the bundle, sent before the supervisor exits;
  - the server down: the report kept in the outbox and sent by the next run;
  - `SENTRY_DSN` set and ours unset: nothing sent, no extra process.
- **LSP:** the stdio harness with the test server: a `didChange` that
  crashes produces one event with the client tags and the overlay text, and a
  repeat produces none.
- **Replay:** a bundle from the end-to-end tests replays, and reduces to a
  bundle that still reproduces; hostile bundles (`..`, absolute paths, links,
  a decompression bomb) are rejected.
- **Supervisor:** exit codes passed through; `SIGTERM` forwarded; on Linux
  the worker ends when the supervisor is killed.
- **Secrets:** a config with `ext.sql.reflection.password` and a `--set` of
  it produce a bundle, a crash state and a recorded command line with neither
  value in them.
- **Broken pipe:** `check-project` over a fixture producing more output than
  a pipe buffer, into a reader that closes after one line, exits 141 with
  nothing on stderr and, with a DSN, sends nothing.
- **Hygiene:** `tests/common::phpstan_rs()` run from a test process whose
  environment has the DSN set starts a child without it; `xtask`'s command
  builders likewise, as a unit test in `xtask`.
- **Windows abort:** the `crash-injection` feature's abort, on the Windows
  CI job, produces the supervisor's "worker terminated" event.
- **Dump scrubbing:** with a canary variable in the environment and a canary
  `--set` value, the injected stack overflow's uploaded minidump contains
  neither, on all three CI operating systems.
- **Cache fallback:** with the cache directory unwritable, a report still
  arrives, from the temporary-directory fallback; with that unwritable too,
  sent directly.
- **Degraded starts:** a supervisor whose worker can't start runs the command
  itself and exits with its result.
- **SQL cache:** a panic injected while the cache lock is held leaves later
  files' SQL reflection working.

The injection hook the unit tests already use (`tests::panic_injection`)
becomes the `crash-injection` feature's hook, compiled under
`#[cfg(any(test, feature = "crash-injection"))]`, so there is one mechanism.
The end-to-end tests live in `crates/phpstan-rs/tests/crash_reporting.rs`
with `required-features = ["crash-injection"]`; `rust-ci.yml` and
`cargo xtask gates`' quick tier run them with
`cargo test -p phpstan-rs --features crash-injection --test crash_reporting`,
and `cargo hack --each-feature` covers the feature's build. Test-only
dependencies: `rcgen` for the test CA, `rustls`'s blocking `ServerConnection`
over a `std::net::TcpListener` for the server (no async runtime in tests),
`tar` and `flate2` to read bundles.

`rust-ci.yml` runs on ubuntu-24.04 only, and the supervisor, minidump and
detached-process code differs per OS, so a `crash-reporting` job runs the
`crash_reporting` test on `macos-26` and `windows-2025-vs2026` too, the
runners `release.yml` already uses.

Step 3 also measures what the feature costs with no DSN: `hyperfine` on
`phpstan-rs --version` and on `check-project` over a small fixture directory,
before and after, must agree within noise, and the commit message records the
release binary's size before and after.

### 4.20 Broken pipes, poisoned caches and environment hygiene

- **Broken pipes.** The CLI's output (`check-project`'s report in every
  format, `check`'s summary, `demo`, `convert-baseline`) is written through a
  locked `stdout` with `write!`, the errors propagated. A `BrokenPipe` error
  ends the run quietly with exit code 141 (128 + `SIGPIPE`, what a shell
  reports for a program the signal ended); any other write error is printed
  and exits 1. This is F3's formatters writing to a `Write` (§5), pulled
  forward for the formats that exist today. Stderr keeps `eprintln!`: a
  closed stderr has no one to tell.
- **Poisoned caches.** `SqlContext::reflect` takes its cache lock with
  `lock().unwrap_or_else(PoisonError::into_inner)`: its critical sections are a
  `get` and an `insert`, which leave the map valid whatever panicked around
  them, so a poisoned lock holds nothing torn. The LSP's `Server` mutex keeps
  `unwrap`: a handler that panicked mid-update can leave the server's state
  inconsistent, and Stage F's snapshots replace the lock.
- **Environment hygiene.** A developer who sets `PHPSTAN_RS_SENTRY_DSN` in
  their shell must not have gate, corpus, performance or test runs supervised
  or reported: the performance gate would time the supervisor too, and a
  test's injected panic would reach the real server. Every place that starts
  the binary removes `PHPSTAN_RS_SENTRY_DSN`, `PHPSTAN_RS_SENTRY_CA_FILE` and
  every `_PHPSTAN_RS_*` variable from the child's environment: `xtask`'s
  `binary()` and `runner()` commands, and one helper,
  `tests/common::phpstan_rs()`, that `tests/cli.rs`, `lsp_exit_code.rs`,
  `version.rs` and the LSP harness use in place of their own
  `Command::new(env!("CARGO_BIN_EXE_phpstan-rs"))`. The crash-reporting tests
  set the variables back explicitly, pointing at their test server.
  `tools/gate-runner` never calls `init`.

### 4.21 README

A "Crash reporting" section, in these words:

> phpstan-rs can send reports of its own crashes to a Sentry server you
> run. It is off by default. Set `PHPSTAN_RS_SENTRY_DSN` to the DSN of a
> Sentry project to turn it on (in VS Code, the
> `phpstan-rs.crashReporting.dsn` user setting). `SENTRY_DSN` is ignored.
>
> A report contains the error message and stack trace, the command line,
> the effective configuration, and a copy of the PHP source files needed to
> reproduce the crash: the file being analysed and the files it depends
> on, up to 10 MiB compressed, including unsaved editor buffers, and the
> schema ext-sql reflected. It contains your file paths, OS and CPU
> architecture; it leaves out your hostname, your environment variables and
> the database connection settings (host, user, password, database name). A
> report of a hard crash (a stack overflow, an abort) also carries a memory
> snapshot of the program's thread stacks, which can hold fragments of
> whatever it was processing at that moment.
> Only turn it on for code you may send to that server.
>
> Reports that can't be sent are kept in the user cache directory
> (`phpstan-rs/crash-outbox` under `$XDG_CACHE_HOME`, `~/Library/Caches`
> or `%LOCALAPPDATA%`) and retried by later runs for 30 days; with
> reporting turned off nothing reads that directory, so delete it by hand to
> discard what's left. With a private CA, point
> `PHPSTAN_RS_SENTRY_CA_FILE` at its certificate. For another editor, set
> the variable in the environment the editor starts `phpstan-rs lsp` with.

## 5. Edits to `CRATE_SPLIT_PLAN.md`

Each goes in with the step that causes it (§6):

- **B8, "No global panic hook":** marked landed early (step 1).
- **F1, "Panic isolation, on the pool":** the single hook and the internal
  error with file, message, location and backtrace marked landed early (step
  1), naming the backtrace as an unresolved `backtrace::Backtrace`; running
  the LSP's handlers on the pool stays in F1, and when it lands each handler
  runs through `crash_report::isolated`, so their panics become internal
  errors reported like the workspace check's, and this plan's unisolated LSP
  path (§4.14) is left for panics outside any handler.
- **F1, "Internal errors are reported in every format":** the text and
  friendly lines and the summary landed early (step 1); JSON, checkstyle and
  JUnit stay.
- **F1, `AnalysisResult`:** its internal-error list is `Vec<InternalError>`
  (§4.4), and each whole-program pass is an isolated unit.
- **F2, result cache location:** it uses `crash_report::cache_dir()` (step 3).
- **F4, Binary:** the hidden `__crash-finish` subcommand and the supervisor
  and worker roles are part of the binary's surface.
- **F3, ErrorFormatter set:** the existing formats write through a `Write`
  with broken pipes ending the run quietly (step 1b); F3's formatters keep
  that contract.
- **§3 crate list:** `crash-context`, below `reflection` and `project`, with
  `backtrace` its only dependency; the engine crates depend on it for
  `isolated` and `focus`.

## 6. Order

One commit per step, each checked by fmt, clippy and its touched modules'
tests; the end-to-end tests run from step 8 on and again at the end.

1. **Single panic hook, isolated units and internal errors** (§4.3 without
   the client paths, §4.4, §4.5 with focus), pulled forward with no Sentry
   involved, in the new `crash-context` crate: the four swaps go, vendor
   discovery's with them; `entry` wraps `main` and the LSP's threads; parse,
   facts, passes, the member-index re-check and the load steps stop dropping
   or escaping panics; the CLI output changes (a
   behaviour commit with its `tests/cli.rs` changes); the §5 edits for B8 and
   F1.
   Then, each its own behaviour commit with its test: **1b** output through
   a writer, broken pipes exiting 141 (§4.20), with the §5 edit for F3;
   **1c** the SQL cache's poison recovery (§4.20).
2. **Build:** `[profile.dist]`, the build-ID flags, the release workflow's
   split, strip, artifact and `upload-debug-files` job (§4.16), the secrets,
   and `docs/crash-reporting-server.md` (§4.17).
3. **Client and outbox:** the `crash-reporting` feature and dependencies,
   roles, `init`, options, the rustls provider and CA file, the outbox
   transport and drain, `cache_dir`, `crash_report::exit`, the `SENTRY_DSN`
   test, the no-DSN cost measurement, the environment hygiene in `xtask` and
   the tests (§4.20); the §5 edit for F2.
4. **Events from internal errors:** grouping, fingerprint, tags, context and
   breadcrumbs, the CLI's sending and kept-for-later lines (§4.6, §4.7,
   §4.13), without bundles.
5. **Bundles:** the builder, the schema's path-key and secret-key registries
   and their test, the config rewrite, `SchemaCatalog::to_ddl`, `sent.json`
   (§4.8).
6. **Pending reports and the finisher:** the crash state, the hook's
   unisolated path, `__crash-finish` (§4.9's crash state, §4.10); the §5 edit
   for F4.
7. **Supervisor** (§4.11): the crash-state handshake, signal forwarding,
   the parent-death signal and the Windows job object.
8. **Minidumps:** the integration, named workers, focus forwarding, overlays
   on disk (§4.9), `crash-injection` and the end-to-end tests.
9. **LSP:** the report thread, dispatch focus and breadcrumbs, client tags,
   session dedupe (§4.14), and its tests.
10. **VS Code:** the setting (§4.15).
11. **`tools/crash-repro`:** replay, reduce, fetch (§4.18).
12. **README** (§4.21).

## 7. Done when

- With the DSN unset, or only `SENTRY_DSN` set, no process, thread or
  connection beyond today's exists, and the test server receives nothing.
- A panic in parse, facts, check or any whole-program pass, in the CLI or the
  LSP, reaches the test server as one event per location with a bundle that
  `crash-repro replay` reproduces, and the rest of the run's results are
  still reported.
- A stack overflow on a worker thread and on the LSP's debounce thread each
  reach it as a fatal event with a minidump and a bundle; for the CLI, before
  the supervisor exits.
- A report made while the server is down reaches it on the next run.
- A release built by the workflow produces events whose frames Sentry shows
  with function, file and line, and source context from the uploaded
  sources.
