# Crash reporting

phpstan-rs reports its own crashes (panics, stack overflows, aborts) to a
self-hosted Sentry, with a bundle of the inputs that reproduce each one. It is
off unless the user sets a DSN. This plan sits beside `CRATE_SPLIT_PLAN.md`:
it pulls Stage F's single panic hook and internal-error record, and B8's
removal of vendor discovery's hook, forward in their final form (§5), the way
`GATE_SPEED_PLAN.md` pulls items forward.

Status: design. Nothing here is implemented.

## 1. Decisions

- **SDK: `sentry` 0.49.3** (getsentry/sentry-rust) for events. A self-hosted
  server works the same as sentry.io; only the DSN differs.
- **Native crashes: `minidumper` 0.10 and `crash-handler` 0.7 directly**, the
  crates under `sentry-minidump`, with our own supervisor process as the
  minidump server (§4.1). `sentry-minidump` and `minidumper-child` would each
  add a re-executed reporter process of their own; the supervisor already
  outlives the worker, so it hosts the server instead. Attaching the crash
  handler takes one `unsafe` call (`crash_handler::make_crash_event`, whose
  closure runs inside the signal handler); it lives in a crate of its own,
  `crates/crash-sys`, behind a safe function, so every other crate keeps
  `unsafe_code = "deny"` (§4.1).
- **Only finisher processes talk to Sentry** (§4.2). The worker doing the
  analysis writes every crash to disk as a pending report and never opens a
  connection, builds a bundle or starts an async runtime.
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
  (§4.12). Every run with a DSN, the LSP included, is a supervisor and a
  worker (§4.1): the supervisor catches the worker's native crashes, keeps
  what it needs to report them, and finishes every report through a finisher
  child it waits for (§4.10, §4.11), so a CI job that ends right after the
  crash still sends the report. Nothing is left running detached.
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
  today (§2); the CLI stops panicking there (§4.20) rather than crash reporting
  learning to ignore it.
- **A `crash-reporting` cargo feature**, on by default, carries all of it.
  Without it `init` is a no-op and `sentry`, `reqwest`, `minidumper`,
  `crash-handler` (with its `pthread_create` interposer), `crash-sys` and the
  supervisor's signal crates aren't linked, for anyone building a binary with no
  network code. `--no-default-features`, which CI and the gates already build,
  drops it with the rest.

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
  runner's hard stop) on every OS. On cgroup v2 (checked here:
  `/sys/fs/cgroup` is `cgroup2fs`, and this shell's cgroup has
  `memory.events` with an `oom_kill` line), the kernel counts OOM kills per
  cgroup, which tells an OOM kill apart. On Windows, Rust's `abort` (an allocation
  failure, a panic while panicking) uses `__fastfail`, which ends the process
  without running exception handlers. On Linux and macOS an abort raises
  `SIGABRT`, which `crash-handler` catches.
- **Cache directory:** none exists yet. Stage F2 names the user cache
  directory (`$XDG_CACHE_HOME/phpstan-rs` or the platform equivalent) for the
  result cache.

## 3. What the crates do (read from their source)

- **`sentry-minidump` and `minidumper-child`** both work by re-executing the
  current binary as a crash-reporter process from inside the app (the
  reporter inherits the app's stdio and environment, the app pings it every
  2.5 s, it exits after 5 s without a ping), and both keep the client side
  (`crash_handler::CrashHandler::attach` with `make_crash_event`) private
  behind that spawn. `make_crash_event` is `unsafe`: its closure runs inside
  the signal handler, and `minidumper-child`'s closure calls only
  `client.ping()` and `client.request_dump(ctx)`.
- **`minidumper` 0.10's server** is public: `Server::with_name(socket)` and
  `run(handler, &shutdown, stale_timeout)`, with a `ServerHandler` trait:
  `create_minidump_file` (we choose where the dump goes),
  `on_minidump_created` (the written dump), `on_message(kind, bytes)`, and
  client connect and disconnect callbacks. The dump is written by the server
  process, reading the crashed process's memory, so a worker that overflowed
  its stack or ran out of memory doesn't need to do any work itself. Messages
  travel over `SOCK_SEQPACKET` on Linux (one message per packet, so its size
  is bounded by the socket buffer) and a stream on macOS and Windows read with
  one `recv` per message (`src/ipc/server.rs:36-70`), so a large message can
  arrive split. Messages are for small things only.
- **A Linux minidump carries the process's environment and command line.**
  `minidump-writer` 0.12 copies `/proc/<pid>/environ` and
  `/proc/<pid>/cmdline` into the `LinuxEnviron` and `LinuxCmdLine` streams
  (`src/linux/minidump_writer/mod.rs:422-423`), next to `auxv`, `maps`,
  `status`, `/proc/cpuinfo` and the OS release file. Sent as-is, a native
  crash in CI would upload every secret in the job's environment (cloud keys,
  tokens, the DSN itself, the variable `password_env` names) and any
  `--set` on the command line. The writer offers no way to leave streams
  out, so the supervisor blanks them in the file it chose for the dump before
  anything else reads it (§4.9).
- **`crash-handler`** interposes `pthread_create` on Linux and macOS, so every
  thread gets its own alternate signal stack and a stack overflow on a rayon
  worker or the debounce thread is caught. The interposer is linked in whether
  or not a DSN is set; it costs one `mmap` and one `sigaltstack` call per
  thread. The signal handlers are installed only by `attach`. On Windows a
  vectored exception handler sees `EXCEPTION_STACK_OVERFLOW`; on macOS `SIGABRT`
  is hooked as well.
- **Hubs are per thread.** The first use of `Hub::current()` on a thread
  other than the main one creates a hub from a snapshot of the main hub's top
  scope (`sentry-core`'s `THREAD_HUB`); breadcrumbs and tags added on the main
  thread afterwards never reach it, which is one reason the worker keeps its
  breadcrumbs in its own process-wide store instead (§4.2).
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
  queue of 30 envelopes (the finisher's client sets its own timeout, §4.2).
- **The server side** (read from getsentry/relay, getsentry/sentry and
  getsentry/self-hosted `master` on 2026-09-28):
  - Relay's default limits (`relay-config/src/config.rs`): 1 MiB per event,
    200 MiB per attachment, for all attachments together and per envelope.
    Self-hosted's nginx caps every request body at 100 MB
    (`client_max_body_size 100m`).
  - Relay rejects no event for its age: a timestamp older than the project's
    event retention (`DEFAULT_EVENT_RETENTION`, 90 days) is replaced by the
    time the event arrived, with a `past_timestamp` error recorded
    (`relay-event-normalization/src/validation.rs`).
  - `sentry:store_crash_reports` defaults to 0 (`lang/native/utils.py`):
    minidumps are symbolicated and then dropped. -1 keeps all, a number keeps
    that many per issue, at most 100. Attachments that aren't crash reports
    are stored regardless (`event_manager.py`).
  - Downloading an attachment
    (`api/endpoints/event_attachment_details.py`) needs an authenticated
    *user* whose organization role reaches `sentry:attachments_role`
    (default `member`, `SENTRY_DEFAULT_ROLE`); an organization auth token has
    no user and is refused. `?download` with any value returns the file,
    possibly as a redirect to the object store, and possibly content-encoded
    as stored when the client accepts that encoding.
  - Organization auth tokens carry `org:ci`, which `ProjectReleasePermission`
    accepts for debug-file uploads (`DifAssembleEndpoint`, `DebugFilesEndpoint`)
    and releases (`api/bases/project.py`).
  - Self-hosted's `sentry.conf.example.py` sets
    `SENTRY_QUOTAS = "sentry.quotas.redis.RedisQuota"`, so client-key rate
    limits are enforced.
- **Dependency policy:** every crate the probe build adds has a license
  `deny.toml` allows (`aws-lc-sys`'s compound expression is already in the
  lock and passes today); `win32job`, `signal-hook` and `rcgen` are MIT OR
  Apache-2.0. `minidump-writer` depends on `nix` 0.30, so the supervisor uses
  `nix` 0.30 too, adding no duplicate. `nix` 0.30 has safe `dup2_stdin` and
  `dup2_stdout` (`src/unistd.rs:446,492`), `set_pdeathsig`, `getppid` and a
  kqueue `EVFILT_PROC`/`NOTE_EXIT` filter.
- **`sentry-cli` 3.8.0** (current on 2026-09-28) has `debug-files upload
  --include-sources`, `debug-files check`, and `releases new`,
  `set-commits`, `finalize`. `sentry-cli-Linux-x86_64` from the 3.8.0 GitHub
  release has SHA-256
  `13f8cb34ae01a6a272d7d7c22e277a105286615b4020de900ea95a8de47cdbb6`; the
  release publishes no checksum file of its own.

## 4. Design

`crates/crash-context` (library name `crash_context`) holds `hook`,
`isolated`, `focus`, `record` (panic records), `carry`, `entry`,
`internal_error` and the `Reporter` trait. `crates/crash-sys` holds the two
calls that need `unsafe` (§4.1). `crates/phpstan-rs/src/crash_report/` holds
`init`, `events`, `bundle`, `state`, `pending`, `supervisor`, `outbox`,
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

Without a DSN a run is one process, as today. With one, every command, `lsp`
included, is two processes of the same binary:

| Role | Started by | Does |
| --- | --- | --- |
| supervisor | the user, CI or the editor | hosts the minidump server, starts the worker, keeps the run's crash state, finishes reports, relays the exit status (§4.11) |
| worker | the supervisor | the real run, with the Sentry client and the crash handler attached |
| finisher | a supervisor, and only as its child | completes one pending report with its bundle and sends it (§4.10) |

`_PHPSTAN_RS_CRASH_ROLE` (`worker` or `finisher`) tells a child its role; a
process with a DSN and no role is a supervisor. The supervisor has no Sentry
client and no network code: it writes pending reports to disk and runs
finishers, which do the sending.

**The worker and the supervisor talk over the minidump socket.** The supervisor
creates `minidumper::Server::with_name` on a fresh socket and runs it on a
thread with our `ServerHandler`, then starts the worker with the name in
`_PHPSTAN_RS_CRASH_SOCKET`. The worker connects a `minidumper::Client` and calls
`crash_sys::attach(client)`. Messages the worker sends, each under 4 KiB (§3):

- `FOCUS` and `FOCUS_CLEAR` with a thread name and a unit's path (§4.7);
- `BREADCRUMB` and `TAG`, mirrored so a native crash carries them;
- `PENDING` with a pending entry's id, after the worker has written that
  entry to disk (§4.6).

Anything larger (the crash state, editor buffers, a pending event) goes
through the run's crash-state directory on disk (§4.9).

**Only the user can reach the socket.** It is a path socket inside a `0700`
directory, never a Linux abstract one: an abstract socket has no permissions,
so any local process could connect, send messages and ask the supervisor to
dump a process. The directory is `$XDG_RUNTIME_DIR/phpstan-rs` when set (a
per-user `tmpfs`), otherwise the run's crash-state directory; when that path
is longer than a socket address holds (108 bytes on Linux and Windows, 104 on
macOS), `std::env::temp_dir()/phpstan-rs-<uid>`, created `0700` and refused if
someone else owns it. The name is `<run id>.sock`, and the supervisor removes
it when it exits. Linux also restricts who may read the worker's memory to the
supervisor (`set_ptracer`, below).

**`crates/crash-sys`** is the one crate with `unsafe`, allowed on its three
functions only (`#[allow(unsafe_code)]`; the workspace lint is `deny`, not
`forbid`):

- `attach(client: Arc<minidumper::Client>) -> Result<CrashHandler, Error>`
  wraps `CrashHandler::attach(unsafe { make_crash_event(..) })`. SAFETY: the
  closure runs in a signal handler and calls only
  `client.request_dump(ctx)`, which `minidumper` provides for that context,
  as `minidumper-child`'s own closure does; it allocates nothing and takes no
  lock. On Linux it then calls `set_ptracer(Some(supervisor_pid))`, so only
  the supervisor may read the worker's memory.
- `release_std_handles()` on Windows closes the process's own copies of its
  standard input and output handles (`GetStdHandle`, `CloseHandle`,
  `SetStdHandle` to null) after the worker has inherited them (§4.11). On Unix
  the same is safe through `nix::unistd::dup2_stdin`/`dup2_stdout` onto
  `/dev/null`, and lives in the supervisor.
- `enter_background()` makes the calling process background work: on Unix
  `setpriority(PRIO_PROCESS, 0, 19)` (`nix` has no wrapper), on Linux also
  `ioprio_set` to the idle I/O class, on macOS also `setiopolicy_np` with
  `IOPOL_THROTTLE`, on Windows `SetPriorityClass` with
  `PROCESS_MODE_BACKGROUND_BEGIN`, which lowers CPU, I/O and memory priority
  together. A finisher calls it first (§4.10), so building a bundle beside a
  live language server, or at the end of a CI job, never competes with
  analysis for the machine.

**Children's stdio.** The worker inherits the supervisor's stdin, stdout and
stderr. A finisher gets null stdin and stdout, and its stderr goes to
`finisher.log` in its pending entry.

**`init` failing** leaves the run without reporting, with one line on stderr:
an invalid DSN, or an unreadable `PHPSTAN_RS_SENTRY_CA_FILE` (then only the
system roots are trusted).

**A process that can't be started degrades** instead of failing the run. The
supervisor failing to start its worker (`current_exe` unavailable, as when the
binary was replaced during an upgrade and `/proc/self/exe` names a deleted
file), or to create its socket, runs the command itself as a worker with no
crash handler, with one line on stderr. A worker that can't connect to the
socket or attach runs without native-crash reports, likewise. A finisher that
can't be started leaves its pending entry for the next supervisor.

### 4.2 Where Sentry runs

**Only finishers talk to Sentry.** The worker, the process doing the
analysis, has no Sentry client, no transport, no async runtime and no
network code: a crash of any kind becomes a pending report on disk (§4.6),
and the supervisor's finisher children (§4.10) build its bundle and send it.
Sending never competes with the analysis for time or memory, bundle building
can't crash the process whose results the user is waiting for, and a report
survives the worker being interrupted.

`main` starts with `crash_report::init()`, before `Cli::parse`, the dhat
profiler and anything else, because a supervisor must start its worker before
any app work and never do that work itself. With `PHPSTAN_RS_SENTRY_DSN`
unset it returns `None` and nothing else in this plan runs. With it set, it
reads the role (§4.1): a supervisor supervises; a worker registers the
`Reporter` (§4.3) and runs; a finisher builds a client with these options:

- `dsn` from the variable; `release` = `phpstan-rs@<version>+<commit>`, the
  commit from `PHPSTAN_RS_BUILD_COMMIT` at compile time (`option_env!`, set by
  the release workflow), `+dev` when absent. A build script reading
  `git rev-parse HEAD` would give development builds their commit, but its
  output changes on every commit and would force the crate, and everything
  above it, to recompile each time; instead every event carries the binary's
  identity, its build ID and its executable's size and modification time as
  the extra `binary`, which tell two development builds apart. The worker
  records them into each pending event, since the finisher is the same binary
  but runs later; `environment` = `release` for a `dist` build
  (`PHPSTAN_RS_DIST` at compile time), `development` otherwise;
- `default_integrations = false`, then the contexts and debug-images
  integrations. Contexts describe the machine, which is the worker's too.
  Debug images must be the worker's (its load addresses), so the worker
  writes its module list into the pending event and the finisher keeps it;
- `send_default_pii = false`; `before_send` clears `server_name`;
- `in_app_include`: the workspace crates (`phpstan_rs`, `syntax`, `types`,
  `phpdoc`, `sql`, `diagnostics`, `composer`, `config`, `stub_sources`,
  `crash_context`), so Sentry folds std, rayon and the SDK out of the trace;
- `transport`: the outbox transport. It writes each event envelope to the
  outbox's `ready/` (§4.12) before sending; SDK-internal envelopes (client
  reports) are sent without being kept. The sender POSTs with its own
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
  stderr;
- `shutdown_timeout` 50 s, inside the supervisor's 60 s wait (§4.11).

**The worker's side of reporting** is `crash_context`'s own state, not a
Sentry hub: breadcrumbs (a ring of the last 100), tags and context live in
process-wide storage that every thread writes to, so a pending event written
from any thread carries the whole run's, and each is mirrored to the
supervisor (`BREADCRUMB`, `TAG`, §4.1) for a native crash, which the worker
can't write. Sentry's per-thread hubs (§3) never come into it. Every
`process::exit` in the worker goes through `crash_report::exit(code)`, which
makes sure pending entries are written before the process ends:
`main.rs:83`, `main.rs:91` and `main/check_project.rs:116,123`.

A user cache directory, `crash_report::cache_dir()` (`dirs::cache_dir()` +
`phpstan-rs`), holds the outbox, the crash state and `sent.json`, and is
created with mode `0700` on Unix, since it holds source. When there is no
user cache directory (no `HOME` in a container) or it can't be written, it
falls back to `std::env::temp_dir()/phpstan-rs-<uid>` (the user's name on
Windows), created `0700` and refused if it exists owned by someone else; when
that fails too, crash reporting is off for the run, with one line on stderr,
since every report passes through the disk. An internal
`_PHPSTAN_RS_CACHE_DIR` overrides the location, for tests running in
parallel. Stage F2's result cache reuses the function (§5).

In a `hotpath` build, the `#[hotpath::main]` wrapper runs before `init`, so a
supervisor or finisher starts a hotpath guard of its own; profiling builds
are never run with a DSN.

### 4.3 Panic hook

One hook, installed once, replaces all four swaps in §2. Three thread-locals
drive it: `isolated` (inside an isolated unit), `focus` (§4.7) and
`reporting` (inside crash-report code, §4.13). `crash_report::isolated(unit,
|| ...)` sets and clears `isolated` around a `catch_unwind`; every site in §2
and every whole-program pass (§4.5) runs through it. A panic record holds the
message, the location, the thread's focus at that moment, and an unresolved
`backtrace::Backtrace`, captured when reporting is on (a DSN) or
`RUST_BACKTRACE` is set and resolved only when printed or written into a
report. The hook:

- **when `reporting` is set** prints the default message and does nothing
  else;
- **on an isolated thread** prints nothing and stores the record in a
  thread-local, which `isolated` takes into the unit's internal error (§4.4);
- **inside a parallel closure** does the same. Rayon carries a worker's panic
  to the thread that called into the pool with `resume_unwind`, which runs no
  hook, so every `par_iter`/`into_par_iter`/`par_chunks` closure (the eight in
  `registry/resolve.rs`, `hierarchy.rs`, `purity.rs`, `stubs.rs`, and the
  per-file ones in `project.rs`, `session.rs` and `main/check.rs`) runs its
  body through `crash_context::carry(|| ..)`: it marks the thread, catches a
  panic on the worker, takes the record, and resumes with a
  `CarriedPanic(record)` payload, which `isolated` and `entry` unwrap. Only
  phpstan-rs uses rayon in this workspace (`cargo tree -i rayon`), so every
  pool closure is one of ours. `clippy.toml`, which already bans randomly
  seeded hash types, gains `disallowed-methods` entries for rayon's
  parallel-iterator constructors (`par_iter`, `into_par_iter`, `par_chunks`,
  `par_bridge`, `join`, `scope`), with the reason "run the body through
  `crash_context::carry`"; each audited site carries
  `#[allow(clippy::disallowed_methods)]`, so a new site can't skip `carry`
  unnoticed;
- **anywhere else** prints the default message (the previous hook, kept) and,
  with reporting on, calls the `Reporter`'s `unisolated(record)`: the app
  crate builds the event (`PanicIntegration::event_from_panic_info`, with the
  backtrace resolved here, where the addresses are valid) and writes it as a
  pending report (§4.6). A location already reported by this process is
  counted, not reported again, and a process makes at most 3 pending reports,
  so a panic that leaves the process panicking in a loop can't flood its
  supervisor.

The hook itself never panics: every step returns a `Result` that it ignores.

A panic that no isolated unit catches reaches the top of a thread.
`crash_report::entry(|| ...)` wraps `main`'s body and every thread the program
spawns (the LSP's reader and debounce threads). When a `CarriedPanic`
escapes, `entry` prints the default message the hook held back and reports it
through `unisolated`, then resumes unwinding, so the process or thread ends as
it does today.

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

Every other format carries them too, and the run fails: Stage F's "Internal
errors are reported in every format" and "The exit code" are pulled forward
whole (§5), since a crash CI can't see is the problem this plan exists for.

- **JSON:** a top-level `internal_errors` array, each
  `{ "path": <path or null>, "unit": "check", "message": ..., "location":
  "file:line:col", "focus": <path or null> }`, `totals.internal_errors`, and
  `"incomplete": true` when there is any. `totals.files_checked` keeps
  excluding every file with an internal error, now including the parse
  panics it used to count as skipped, so its value doesn't change.
- **Checkstyle:** an `<error severity="error" source="phpstan-rs.internal"
  message="internal error while <unit>: <message> (at <location>)"/>`
  without a line, under the file's `<file>`; a pass or load error goes under
  its focus file, or under the config file (the working directory when there
  is none) without a focus.
- **JUnit:** one failing `<testcase classname="phpstan-rs.internal"
  name="<unit> <path>">` per internal error, its `<failure>` holding the
  message and location.
- **Text and friendly:** the summary adds "the result is incomplete".
- **Exit code** (the Stage F item as written, with its approved delta in
  `tests/cli.rs`): 0 when nothing remains after the post-filters; 1 when a
  file or not-file error remains; 2 when the run couldn't start (an invalid
  config, no files under the paths, an unreadable baseline) or finished
  incomplete (an internal error). Today every run exits 0 except a config or
  `--set` error (1). The supervisor passes the code through (§4.11).

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
carries it in its record through `carry`. A failed step leaves the registry
unusable, so the run ends with that internal error rather than analysing
against half-resolved declarations. The LSP's incremental resolution
(`project.rs:987`, on the debounce thread) stays unisolated until Stage F
moves it onto the pool; its panics are reported through `entry` (§4.3).

### 4.6 From crashes to pending reports to events

Every crash becomes a pending report the moment it happens, and every report
is finished the same way (§4.10). A pending report is a directory,
`<cache>/crash-outbox/pending/<entry id>/`, holding `report.json` (what the
worker knows: the unit, message, location, focus, the resolved stack frames,
the worker's module list, tags, context and breadcrumbs, the binary's
identity) or, for a native crash, the dump (§4.9), and `state/`, a snapshot
of the run's crash state. For a panic the worker writes `state/` itself,
before it sends `PENDING`: it is alive and knows the state the panic happened
in (`state.json` and every open buffer's text as it was then), where a copy
made by the supervisor after the message could already hold a later edit.
For a native crash the supervisor copies the crash-state directory, which the
dead worker can no longer change.

**For a panic the worker also writes the bundle's plan**, `plan.json`: the
walk of §4.8 run against the live registry, every file up to the cap in
walk order with its distance, role, size and content hash, and the text of
every file an open buffer holds. It is lookups in a project already in
memory, done once per new location under `reporting` (§4.13). With a plan,
the finisher only reads the listed files; it never loads the project, so a
finisher beside a language server costs the files' size, not a second
workspace. Only a native crash or an OOM kill, which leaves no live registry,
has the finisher load the project (§4.10).

- **Isolated panics:** `isolated` hands each internal error to the
  `Reporter` as well as returning it, and the worker writes its pending
  report there and then and sends `PENDING` (§4.1). A run interrupted right
  after still has it on disk. A location already written by this process is
  added to that report's file list instead of making another.
- **Unisolated panics:** the same, from the hook or `entry` (§4.3).
- **Native crashes:** the supervisor writes the report around the dump
  (§4.9).
- **The supervisor** starts finishers: for a CLI command one finisher
  for all of the run's reports once the worker has ended, so the project is
  loaded once however many locations crashed; for `lsp`, a finisher per
  report as it arrives, at most one at a time, since the session goes on.
  Within a supervisor's life, a location already reported is counted into the
  first report, not finished again.
- **One event per location.** A report's event carries the message and stack
  trace of its first crash, fingerprint `["panic", <location>]` (stable across
  symbolicated and unsymbolicated builds), tags and context (§4.7), the file
  count and first 20 paths as extras (Sentry trims large extras; the full list
  goes in the bundle's manifest), and a bundle (§4.8) for its smallest file,
  the cheapest to reproduce.
- **Messages:** an event's message is the panic message cut at 8 KiB on a
  character boundary (a `format!`ed panic can embed a whole AST or type); the
  full message goes into the bundle's manifest. The CLI's own line keeps the
  whole message.
- **Across runs:** `<cache>/crash-reports/sent.json` records a bundle key,
  `(release, panic location, crashing file's content hash)` for a panic and
  `(release, signal or exception code, in-progress files' content hashes)`
  for a native crash, for each bundle the server accepted (§4.2). A repeat
  within 30 days still sends the event, so Sentry's counts stay true, but
  tagged `bundle = already_sent` and without the bundle. The file is updated
  under `File::lock` (std) and replaced by rename, so concurrent runs don't
  lose entries.

### 4.7 Tags, context, breadcrumbs and focus

- **Tags:** `command` (`check`, `check-project`, `lsp`), `level`, `unit`
  (`parse`, `facts`, `check`, `pass:<name>`, `load:<step>`, `native`, `oom`), `pcre_validate`,
  `lsp.client` and `lsp.client_version` (from `initialize`'s `clientInfo`),
  `bundle` (`attached`, `already_sent`, `too_large`, `no_focus`, `failed`,
  `pending`).
- **Context `run`:** analysed and context file counts, the enabled optional
  rules, the PHP version range, whether ext-sql reflects a live database.
- **Breadcrumbs:** each run phase (config, discovery, load, property-assign
  facts, check, each whole-program pass, report); in the LSP each request and
  notification's method, file and position, never its text. The SDK keeps the
  last 100; `crash_report::breadcrumb` also sends each one to the supervisor
  (§4.2), which keeps the last 100 too.
- **Focus:** `crash_report::focus(path)` sets the thread's current file. The
  per-file units set it from their path; whole-program passes as they move
  between declarations (a class's or function's declaring file, a composer
  manifest); the LSP dispatch from the request's document, and the debounce
  worker from the file it rechecks. A worker also sends each unit's focus to
  the supervisor (`FOCUS`), keyed by thread name; the worker pool's threads
  are named `phpstan-rs-worker-<n>` for this (`ThreadPoolBuilder::
  thread_name`). `isolated` clears it when its unit ends (`FOCUS_CLEAR`), so
  an idle thread never names a file it has finished.
- **Two levels of focus.** A unit's focus (the file of a per-file unit, the
  current pass or load step) is sent to the supervisor; a declaration's
  focus inside a pass or load step (one per class, tens of thousands in a
  large project) stays in the thread-local, where the in-process record
  reads it. Setting either costs a thread-local write; a unit's also costs one
  socket message when a supervisor is listening, so a native crash names the file
  a unit was on and a panic names the declaration too. Step 6 measures
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
  - `native` and `oom`: every in-progress file (the dump alone doesn't say
    which thread caused it, and an OOM kill leaves no dump), each walked as
    `check`.

  Then the user's stub files (`stub_paths`) and `composer.json` and
  `composer.lock` from each discovered root (they set the PHP version range).
  The walk fills the bundle up to the cap rather than stopping at a depth.
- **Cap:** 10 MiB compressed or 64 MiB uncompressed, whichever comes first.
  A self-hosted instance accepts request bodies up to 100 MB (its nginx
  `client_max_body_size`, in front of Relay's own 200 MiB limits, §3), but a
  CLI run waits for the upload; 10 MiB of gzipped PHP is roughly 60 MiB of
  source. A crashing file larger than the cap alone is sent as the manifest
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
- **Checked before it's sent.** A bundle is replayed before it goes out: its
  files loaded into a fresh project with its config, its crashing unit run
  under `isolated`, and the result compared with the crash (the same panic
  location; for a native crash, the child running it dying of the same
  signal). The finisher replays in a child
  (`phpstan-rs __crash-replay <bundle>`, hidden like `__crash-finish`), so a
  replay that aborts can't take the report with it. A bundle that doesn't
  reproduce is rebuilt with its walk widened (the next reference distance;
  for a pass or load unit, every analysed file) and replayed again, at most
  three times within the cap. The event is tagged `reproduces` = `yes`, `no`
  or `unchecked` (a live SQL connection the bundle can't carry, a replay
  that timed out after 60 s), so a maintainer knows before downloading
  whether the bundle alone is enough. Replay runs only phpstan-rs's analysis
  over the files, never the project's code, and costs one check of the
  crashing unit per unique crash (`sent.json` skips repeats).

### 4.9 Crash state and minidumps

- **Crash state:** the supervisor creates `<cache>/crash-state/<run id>/`
  (created `0700`), holds `supervisor.lock` in it for its whole life, passes
  its path to the worker in `_PHPSTAN_RS_CRASH_STATE`, and deletes it when it
  exits. The worker writes `state.json` into it: a format version, the
  command line, working directory, project root, config path and effective
  config table, secret keys dropped (§4.8); in the LSP also the workspace
  folders, each folder's config path and table, the client's
  `initializationOptions` and `PhpstanRsSettings`, and `clientInfo`, rewritten
  when they change. The LSP writes an overlay into `overlays/` when `didOpen`
  brings text that differs from the file on disk (an editor restoring an
  unsaved buffer) and whenever the debounce worker takes a change, and removes
  it on `didSave` and `didClose`. A directory whose `supervisor.lock` can be
  taken belongs to a supervisor that was killed outright; the next supervisor
  removes it (§4.12).
- **The dump:** the supervisor's `create_minidump_file` creates the pending
  entry `<cache>/crash-outbox/pending/<entry id>/` and returns
  `minidump.dmp` inside it, so the dump is written straight into the entry.
- **Scrubbing it:** `on_minidump_created` blanks the data of the
  `LinuxEnviron` (`0x47670007`) and `LinuxCmdLine` (`0x47670006`) streams in
  place (the header's stream count and directory offset, then each 12-byte
  directory entry's type, size and offset: no parser crate needed) before
  anything else reads the file. The macOS and Windows writers produce no such
  streams (§3). What a dump still holds is each thread's stack memory, which
  can include fragments of whatever the program was handling (source text,
  and during a live SQL connection its credentials); the README says so
  (§4.21), and the tests check that a canary environment variable and a
  canary `--set` value never appear in an uploaded dump, on Linux, macOS and
  Windows.
- **Completing the entry:** the handler then copies `state.json`, the
  overlays and the threads' last `FOCUS` into the entry's `state/`, writes
  `meta.json` (`unit = native`, the run id, the last 100 mirrored breadcrumbs
  and tags), and asks the supervisor's main thread to finish it once the
  worker has exited (§4.10). No Sentry event exists yet: the finisher builds
  it, a fatal event with the dump as an `event.minidump` attachment, from
  which the server derives the crash itself.

### 4.10 Finisher

`phpstan-rs __crash-finish <pending entry>`, a subcommand hidden from
`--help` (`#[command(hide = true)]`), always a supervisor's child:

1. calls `crash_sys::enter_background()` (§4.1), then takes the entry's
   `entry.lock` with `File::try_lock`, and exits if another finisher holds
   it;
2. increments the entry's attempt count on disk before anything that could
   crash;
3. builds the event: from the worker's `report.json` for a panic; for a
   native crash or an OOM kill, a fatal event carrying `meta.json`'s
   breadcrumbs and tags, and the scrubbed dump when there is one;
4. checks `sent.json` (§4.6): a report whose bundle was already sent within
   30 days goes out without one, tagged `already_sent`, skipping 5 and 6;
5. on the first two attempts, and when the crash state's format version is the
   finisher's own, builds the bundle (§4.8) and replays it in a `__crash-replay`
   child. With a `plan.json` (a panic, §4.6) it reads the planned files and the
   recorded buffer texts, widening along the plan's later distances when the
   replay doesn't reproduce; a file whose content hash no longer matches the
   plan is marked `changed_since_crash`. Without one (a native crash, an OOM
   kill) it loads the project from the entry's `state/` first (parse and index
   only, no checking: `Session` from `state.json` for `check-project`; for
   `lsp`, `lsp::load_workspace(folders, settings, overlays) -> Project`, the
   load `Server::add_workspace_folders` does today moved into a function the
   server and the finisher both call, with no transport; nothing for `check`,
   whose bundle is the file alone) and walks it;
6. on a third attempt, or when the load or build fails, attaches the crashing
   files and the effective config directly, tagged `bundle = failed`, with the
   failure as an extra;
7. writes the finished envelope into the outbox's `ready/` (§4.12), removes
   the pending entry, and sends it with its own client and the outbox
   transport.

A finisher that crashes while loading leaves the entry for the next attempt,
so a project that crashes the loader can't loop forever. It runs without a
crash handler of its own: a finisher's crash is what the attempt count is
for.

### 4.11 Supervisor

The process the user, CI or the editor started, whenever a DSN is set:

1. creates the crash-state directory and the minidump server (§4.1, §4.9);
2. starts the worker (`std::env::current_exe`, the same arguments through
   `std::env::args_os` so non-UTF-8 ones pass exactly, inherited stdio, and
   the role, socket, run id, crash-state path and its own pid in the
   environment);
3. releases its own standard input and output (`nix::unistd::dup2_stdin`
   and `dup2_stdout` onto `/dev/null` on Unix, `crash_sys::release_std_handles`
   on Windows), so the worker holds the only copies: when the worker ends, a
   reader of its stdout (an editor's language client, a pipe) sees the end of
   the stream at once, not when the supervisor finishes its reports. Stderr
   stays, for the supervisor's own lines;
4. starts a draining finisher when earlier runs left anything in the outbox
   (§4.12);
5. waits for the worker, starting a finisher for each `PENDING` message as it
   arrives;
6. when the worker has ended: finishes the native crash's entry if a dump was
   written; after an abnormal end with no dump (on Windows a `__fastfail`
   abort, exit code `0xC0000409`; anywhere, a crash the handler missed),
   creates an entry itself, `unit = native` with the crash state, the threads'
   last `FOCUS` as the in-progress files and the mirrored breadcrumbs, "worker
   terminated by `<signal or exception code>`";
7. after a `SIGKILL`, reports it only when it was the OOM killer. On Linux
   the supervisor reads its cgroup's `memory.events` (cgroup v2: the path
   from `/proc/self/cgroup`'s `0::` line under `/sys/fs/cgroup`) when it
   starts the worker and when the worker ends; the kernel counts in
   `oom_kill` every process of the cgroup killed by any OOM killer, the
   system-wide one included. A worker killed by `SIGKILL` while `oom_kill`
   rose was out of memory: that is a crash worth a report, `unit = oom`, with
   the in-progress files, breadcrumbs and crash state as above, and the
   cgroup's `memory.max` and `memory.peak` as context. Every other
   termination from outside (`SIGKILL` without an OOM kill, `SIGTERM`,
   `SIGINT`, `SIGHUP`) is a user's or a CI runner's decision and isn't
   reported. Without cgroup v2 (macOS, Windows, cgroup v1) an OOM kill can't
   be told from `kill -9`, so a `SIGKILL` isn't reported there. Another
   process in the same cgroup being OOM-killed at the same moment could be
   mistaken for the worker; the worker being the one killed by `SIGKILL` in
   that window makes it very likely it was the one;
8. waits for its finishers, up to 60 s in all, printing
   `phpstan-rs: sending the crash report to <host>` once it starts waiting on
   one; a `SIGINT` or `SIGTERM` during the wait ends it at once, and what is
   unfinished stays in `pending/` for the next supervisor (in the LSP, the one
   the editor starts next, straight away);
9. deletes the crash-state directory and exits with the worker's code, or
   after a crash with 128 + the signal number on Unix and the worker's
   exception code on Windows.

The worker must not outlive the supervisor. On Unix the supervisor forwards
`SIGTERM`, `SIGHUP` and `SIGINT` to it while it runs (`signal-hook`'s
iterator and `nix::sys::signal::kill`, no `unsafe`), so a CI timeout or an
editor stopping the server still stops the work. On Linux the worker sets
`PR_SET_PDEATHSIG` to `SIGTERM` (`nix::sys::prctl::set_pdeathsig`), then checks
that `getppid` is still the supervisor's pid and exits if not, since the
supervisor may have died before the call. On macOS, which has no parent-death
signal, a worker thread waits on a kqueue `EVFILT_PROC`/`NOTE_EXIT` event for
the supervisor's pid (`nix::sys::event`) after the same `getppid` check. On
Windows the worker is started in a job object with kill-on-close
(`win32job`), so ending the supervisor ends the worker.

**The supervisor's own failures never end the worker.** Everything it does
besides waiting (the minidump server thread and its handler callbacks,
writing entries, starting finishers) runs under `catch_unwind`, and a failure
there costs reporting, not the run: a server thread that dies leaves the
worker's crash handler unable to request a dump, so a later native crash ends
the worker as it would without reporting, and the supervisor reports it
through the no-dump path (step 6 above). Only the waiting, the signal
forwarding and relaying the exit status are essential, and they use no
reporting code.

**Container and service environments:**

- **PID 1.** In a container started as `docker run <image> phpstan-rs ...`,
  the supervisor is PID 1. The kernel gives PID 1 no default signal actions,
  so an unhandled `SIGTERM` would do nothing; the supervisor handles and
  forwards them (above), so `docker stop` stops the run. PID 1 also inherits
  every orphan in the container, so as PID 1 the supervisor reaps any child
  it didn't start (`waitpid(-1)` in its wait loop) and no zombie piles up.
- **Process limits.** A cgroup's `pids.max` counts threads, and the worker
  runs one per CPU; the supervisor and a finisher add two processes and a
  replay a third. A spawn refused for the limit degrades as above.
- **No console.** A Windows service or a detached process has no standard
  handles; `release_std_handles` treats a missing or invalid handle as
  already released.
- **ptrace refused.** A sandbox that forbids `ptrace` (gVisor, a strict
  seccomp profile) makes the dump fail; the supervisor then reports through
  the no-dump path.

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
format change keeps. At start, the supervisor removes crash-state
directories whose `supervisor.lock` is free and, when `ready/` or `pending/`
holds anything, starts one finisher (`__crash-finish --drain`) that sends the
`ready/` entries and finishes the `pending/` ones whose lock is free, while
the worker runs.
Entries older than 30 days are deleted, and the oldest go first once the outbox
passes 100 MiB. Relay rejects no event for its age: one older than the project's
event retention (90 days by default) has its timestamp replaced by the time it
arrived (§3), so within 30 days a retried event keeps the time of its crash.

### 4.13 Failures in the reporting itself

- Writing a pending report in the worker runs with `reporting` set and under
  `catch_unwind`: a panic there is printed and the report is left as far as it
  got; the supervisor finishes what's on disk, tagged `bundle = failed`.
- A finisher's own panic or crash leaves its entry for the next attempt; the
  attempt count bounds the retries (§4.10).
- The server unreachable: the envelope stays in the outbox's `ready/`, and
  the supervisor prints
  `phpstan-rs: N crash report(s) kept in <outbox> for the next run`.
- A rejected envelope (4xx other than 429): deleted, with
  `phpstan-rs: the crash-report server rejected a report (<status>)` on
  stderr.
- A run interrupted (Ctrl-C, `SIGTERM`) keeps every report written so far on
  disk; the next supervisor finishes them.
- A report that can't be written (a full disk, a quota) is lost, with
  `phpstan-rs: couldn't keep a crash report: <error>` on stderr; the run
  itself carries on, since every report passes through the disk.

### 4.14 LSP specifics

- **Focus and breadcrumbs:** `run_dispatch_loop` sets the focus and adds a
  breadcrumb per message before `handle_request`, `handle_notification` or
  `queue_change`; the debounce worker does the same per recheck.
- **Handlers' work on the pool, isolated where no state is mutated** (Stage
  F's "Panic isolation, on the pool", §5): the workspace scan, `didOpen`, the
  debounced `didChange` recheck, watched-file events, folder removal, and the
  requests that parse or check (hover, inlay hints, code actions, completion
  probes, references' first member index) run on `project::worker_pool`'s
  64 MiB stacks, so the files that abort the server today (60 nested arrays on
  `didChange`, 120 in the startup scan) are checked there like `check-project`
  checks them. An update has three phases (`update_file_cancellable`,
  `project.rs:897`): parsing, before anything is unindexed; unindexing,
  reindexing and resolving the registry; checking the affected files. Parsing
  and checking mutate nothing and run as isolated units: a panic there is the
  file's internal error, a request that hit one answers a `ResponseError`
  (`InternalError`, -32603), and the server carries on with consistent state.
  The registry phase is not isolated: a panic halfway leaves declarations
  half-updated, and the only proper recovery is C4's snapshots, which let
  Stage F discard the update and keep the last good state. Until then that
  panic stays a crash, reported through `entry` (§4.3), and the editor
  restarts the server; no stopgap recovery is written to be thrown away.
- **Supervised like every command.** The editor starts the supervisor, which
  hands the protocol pipes to the worker and lets go of its own copies
  (§4.11), so the editor sees the stream end the moment the worker does and
  restarts the server without waiting for the crash report; the next
  supervisor finishes whatever the previous one couldn't. An editor that
  stops the server signals the supervisor, which forwards it.
- **Client:** `initialize` sets the `lsp.client` tags and writes `clientInfo`
  into the crash state.

### 4.15 VS Code

- A setting `phpstan-rs.crashReporting.dsn` (string, empty by default,
  `"scope": "machine"`, so only user settings on that machine can set it: no
  workspace's `.vscode/settings.json` can, and Settings Sync doesn't carry it).
  Its description says what reports hold.
- `serverOptions.options.env` passes it as `PHPSTAN_RS_SENTRY_DSN` when set,
  over `process.env`.
- The `onDidChangeConfiguration` handler (`extension.ts:133`) restarts the
  client when it changes, as it does for `serverPath`.
- In a remote window (SSH, a container, WSL) the server runs on the remote
  machine, and a `machine`-scoped setting is read from that machine's
  settings, so reporting is turned on where the analysis runs.

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
  event per location per run). Self-hosted enforces key rate limits through
  its Redis quota backend (§3);
- **storing minidumps:** the project's `sentry:store_crash_reports` ("Store
  Minidumps As Attachments") set to 10 per issue. Its default, 0, symbolicates
  a dump and then discards it, which would leave nothing to open in a
  debugger. The bundle is an ordinary attachment and is stored whatever the
  setting;
- **Security & Privacy:** the data scrubber and default scrubbers left on.
  They match sensitive key names and value patterns in the event, none of our
  keys match, and they don't read plain attachments. "Prevent storing of IP
  addresses" on;
- **attachments access:** the organization's `sentry:attachments_role`
  raised from its default, `member`, to `manager`, so only maintainers can
  download bundles and dumps;
- **grouping:** the default, plus the client's `["panic", location]`
  fingerprint for panics; native crashes group by their symbolicated stacks;
- **alerts:** a new issue and a regressed issue each notify by email;
- **retention:** the instance's `SENTRY_EVENT_RETENTION_DAYS` (90 by
  default) also bounds how long bundles are kept;
- **an organization auth token** for the release workflow, stored as
  `SENTRY_AUTH_TOKEN`: its `org:ci` scope is accepted by the debug-file
  upload and release endpoints (`ProjectReleasePermission`, §3);
- **a personal auth token** for each maintainer who runs `crash-repro fetch`
  (§4.18): downloading an attachment checks the requesting *user's*
  organization role against `sentry:attachments_role`, and an organization
  token has no user, so it is refused.

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
  with `sentry-cli debug-files upload --include-sources`, reading
  `SENTRY_URL`, `SENTRY_ORG`, `SENTRY_PROJECT` and `SENTRY_AUTH_TOKEN` (an
  organization token, or a personal one with `project:releases`), for a
  maintainer running development builds with a DSN.
- **`fetch <issue>`** downloads an issue's latest bundle through the Sentry
  API with `SENTRY_URL`, `SENTRY_ORG`, `SENTRY_PROJECT` and
  `SENTRY_FETCH_TOKEN`, a personal auth token with `event:read` and
  `project:read` whose user holds the attachments role (§4.17):
  `GET /api/0/organizations/<org>/issues/<id>/events/latest/` for the event
  id, `GET /api/0/projects/<org>/<project>/events/<event>/attachments/` for
  the list, then the `repro.tar.gz` entry's
  `.../attachments/<attachment id>/?download=1`. The download may redirect to
  the instance's object store, so redirects are followed (the token isn't
  sent across hosts), and the request sends `Accept-Encoding: identity`, since
  the server otherwise may answer with the stored blob's own content
  encoding.
- **Bundles are untrusted input**, since anyone with the DSN can send one.
  Extraction rejects absolute paths, `..` components, links and device
  entries, and stops past 256 MiB extracted; a bundle is only ever extracted
  into a fresh temporary directory, and replay never runs anything but
  phpstan-rs on it.

### 4.19 Tests

- **Unit:** the hook's four paths; `isolated` recording location, backtrace
  and focus; a panic inside a `par_iter` under an isolated unit recorded once
  through `carry`, with the worker's location and focus; `entry` printing and
  reporting a carried panic that escapes; a pending report's `report.json`
  from each kind of crash; the finisher's event built from it, with the
  `sentry` `test` feature capturing events in memory; the bundle
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
- **Supervisor:** exit codes passed through; `SIGTERM` forwarded; the worker
  ends when the supervisor is killed (Linux, macOS, Windows); a reader of the
  worker's stdout sees the stream end as soon as the worker exits, while the
  supervisor is still finishing a report; `crash-sys` is the only crate whose
  source contains `unsafe` (a test over the workspace's sources).
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
  itself and exits with its result; a supervisor whose minidump server thread
  panics (injected) still relays the worker's exit status, and reports a later
  crash through the no-dump path.
- **Socket:** the socket is a path in a `0700` directory on every OS, never
  a Linux abstract name.
- **Background:** a running finisher's niceness is 19 on Unix and its
  priority class is background on Windows.
- **Snapshot race:** an overlay edited right after an isolated panic doesn't
  reach that panic's bundle, which holds the text the panic saw.
- **Plan:** a panic's finisher builds its bundle from `plan.json` without
  loading the project (its peak memory stays near the bundle's size), and
  widens along the plan when the first replay doesn't reproduce.
- **OOM:** under a cgroup with a memory limit (`systemd-run --user --scope -p
  MemoryMax=...` where the runner delegates one; the test says so and skips
  where none is available), `crash-injection`'s unbounded allocation ends in
  an `oom` report with the in-progress file; a plain `kill -9` of the worker
  produces none.
- **PID 1:** the supervisor run as a PID namespace's init (`unshare --user
  --map-root-user --pid --fork --mount-proc` in the test, no root and no
  container runtime needed; skipped with a printed reason where user
  namespaces are disabled) stops on `SIGTERM` and leaves no zombies.
- **SQL cache:** a panic injected while the cache lock is held leaves later
  files' SQL reflection working.
- **Formats and exit code:** an injected check panic appears in the JSON's
  `internal_errors`, checkstyle's `phpstan-rs.internal` error and a failing
  JUnit case, and the run exits 2; a run with only findings exits 1, a clean
  one 0.
- **Replay:** a bundle whose first walk misses a needed declaration is
  widened until it reproduces and tagged `reproduces = yes`; one depending on
  a live SQL connection is tagged `unchecked`.
- **LSP on the pool:** a file with 120 nested arrays in the startup scan and
  60 on `didChange` is checked without aborting; an injected panic in hover
  answers `InternalError` and the server keeps serving; one in a
  `didChange`'s check phase becomes the file's internal error and the next
  diagnostics match a fresh server's; one in its registry phase ends the
  worker with a report and the editor's restart.

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
standard-handle code differs per OS, so a `crash-reporting` job runs the
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
- **F1, "Panic isolation, on the pool":** marked landed early (steps 1 and
  1f): the single hook, the internal error with file, message, location and
  backtrace (an unresolved `backtrace::Backtrace`), and every LSP handler's
  work on the pool with its parse and check phases isolated. F1 keeps
  isolating the registry phase of an update, which needs C4's snapshots to
  recover from (§4.14).
- **F1, "Internal errors are reported in every format":** marked landed
  early whole (step 1d), with the JSON, checkstyle and JUnit shapes of §4.4.
- **F1, "The exit code":** marked landed early whole (step 1e).
- **F1, `AnalysisResult`:** its internal-error list is `Vec<InternalError>`
  (§4.4), and each whole-program pass is an isolated unit.
- **F2, result cache location:** it uses `crash_report::cache_dir()` (step 3).
- **F4, Binary:** the hidden `__crash-finish` and `__crash-replay`
  subcommands and the supervisor, worker and finisher roles are part of the
  binary's surface; with a DSN every command runs as a supervisor and a
  worker.
- **F3, ErrorFormatter set:** the existing formats write through a `Write`
  with broken pipes ending the run quietly (step 1b); F3's formatters keep
  that contract.
- **§3 crate list:** `crash-context`, below `reflection` and `project`, with
  `backtrace` its only dependency; the engine crates depend on it for
  `isolated` and `focus`. `crash-sys`, used by the app crate only, the one
  crate allowed `unsafe` (three functions).

## 6. Order

One commit per step, each checked by fmt, clippy and its touched modules'
tests; the end-to-end tests run from step 6 on and again at the end.

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
   **1c** the SQL cache's poison recovery (§4.20); **1d** internal errors in
   the JSON, checkstyle and JUnit formats (§4.4); **1e** the exit code, with
   its approved `tests/cli.rs` delta (§4.4); **1f** the LSP's handlers on the
   pool with their parse and check phases isolated (§4.14), with the edit
   tests covering a handler panic; the §5 edits for F1 with each.
2. **Build:** `[profile.dist]`, the build-ID flags, the release workflow's
   split, strip, artifact and `upload-debug-files` job (§4.16), the secrets,
   and `docs/crash-reporting-server.md` (§4.17).
3. **Supervisor:** the `crash-reporting` feature and dependencies, `init` and
   the roles, `cache_dir`, the process pair for every command, the minidump
   server and socket with our messages, the crash-state directory and its
   `state.json`, releasing the standard handles, signal forwarding, the
   parent-death signal, the kqueue watch and the Windows job object, degraded
   starts, the `crash-sys` crate with `release_std_handles` (§4.1, §4.9's
   crash state, §4.11); the `SENTRY_DSN` test, the no-DSN cost measurement,
   the environment hygiene in `xtask` and the tests (§4.20); the §5 edits for
   F2 and F4.
4. **Pending reports, finisher and sending:** `report.json` from isolated
   panics, the hook's unisolated path and `entry`, `PENDING`, the supervisor's
   finisher scheduling and grouping, `__crash-finish`, the finisher's client
   with the rustls provider, the CA file and the outbox transport and drain,
   fingerprint, tags, context and breadcrumbs, `crash_report::exit`, the
   supervisor's sending and kept-for-later lines (§4.2, §4.6, §4.7, §4.10,
   §4.12, §4.13), without bundles.
5. **Bundles:** the builder, the schema's path-key and secret-key registries
   and their test, the config rewrite, `SchemaCatalog::to_ddl`, `sent.json`,
   `__crash-replay` and widening (§4.8).
6. **Minidumps:** `crash_sys::attach`, named workers, focus forwarding, the
   dump written into its entry and scrubbed, the no-dump abnormal end (§4.9,
   §4.11), `crash-injection` and the end-to-end tests.
7. **LSP:** dispatch focus and breadcrumbs, client tags, overlays and state
   updates, per-report finishers with session dedupe (§4.6, §4.14), and its
   tests.
8. **VS Code:** the setting (§4.15).
9. **`tools/crash-repro`:** replay, reduce, fetch (§4.18).
10. **README** (§4.21).

## 7. Done when

- With the DSN unset, or only `SENTRY_DSN` set, no process, thread or
  connection beyond today's exists, and the test server receives nothing.
- A panic in parse, facts, check or any whole-program pass, in the CLI or the
  LSP, reaches the test server as one event per location with a bundle that
  `crash-repro replay` reproduces, and the rest of the run's results are
  still reported.
- A stack overflow on a worker thread reaches it as a fatal event with a
  minidump and a bundle; for the CLI, before the supervisor exits. The LSP's
  handlers no longer overflow at the depths that abort it today.
- An internal error makes every output format say so and the run exit 2.
- A report made while the server is down reaches it on the next run.
- A worker killed by the OOM killer under cgroup v2 reaches it as an `oom`
  report naming the files in progress; a `kill -9` doesn't.
- With a DSN, every command runs as a supervisor and a worker, the only
  `unsafe` in the workspace is `crash-sys`'s three functions, and nothing a run
  starts outlives its supervisor.
- A release built by the workflow produces events whose frames Sentry shows
  with function, file and line, and source context from the uploaded
  sources.
