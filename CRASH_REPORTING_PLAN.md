# Crash reporting

phpstan-rs reports its own crashes (panics, stack overflows, aborts, OOM
kills) to a self-hosted Sentry, each with a bundle of the inputs that
reproduces it. Reporting is off unless the user sets a DSN, and with it off
nothing in this plan runs.

The plan sits beside `CRATE_SPLIT_PLAN.md`. Where the proper form of
something it needs is already designed there, the step pulls that item
forward in its final form rather than landing a stopgap Stage C–F would
replace, as `GATE_SPEED_PLAN.md` does. §9 lists the edits that record each
item landing early.

Status: design. Nothing here is implemented.

## 1. Rules

- One commit per step (§8), checked before the next starts: fmt, clippy, the
  unit tests of the touched modules, and the integration tests the step
  names. The whole test suite, the fixture gate and the end-to-end crash
  tests (§7) run at the end of step 1, from step 6 on, and at the end.
- A step that changes output lands that change as its own behaviour commit,
  with its test changes as the approved delta. Every other commit leaves
  corpus, type, baseline and fixture snapshots byte-identical.
- No stopgap. When the proper fix depends on something not yet built (the
  LSP's registry phase on C4's snapshots, §6.1.6), the code stays as it is
  and the crash is reported, rather than an interim fix being written to be
  thrown away.
- `unsafe` stays denied workspace-wide. The three calls that need it live in
  one crate, `crash-sys`, behind safe functions (§6.2.3).

## 2. Decisions

- **Opt-in by `PHPSTAN_RS_SENTRY_DSN`, never `SENTRY_DSN`.** `sentry::init`
  falls back to `SENTRY_DSN`, `SENTRY_RELEASE` and `SENTRY_ENVIRONMENT` for
  any option left unset, and a PHP project's CI or container often sets
  `SENTRY_DSN` for the application itself; reading it would send
  phpstan-rs's crashes, with the project's source, to the application's
  Sentry. phpstan-rs reads only its own variable and sets `dsn`, `release`
  and `environment` explicitly, so no fallback applies. No DSN is compiled
  into any build.
- **Only the environment turns it on.** No `phpstan-rs.toml` key sets the
  DSN: a config file travels with a repository, and a cloned repository must
  not be able to send its reader's crashes anywhere. The VS Code setting is
  `machine`-scoped for the same reason (§6.10).
- **Source is sent as-is.** The user opted in, and a report that reproduces
  beats one that protects the code; nothing is anonymised, hashed or
  scrubbed. The README and the VS Code setting say exactly what a report
  holds (§6.14).
- **What never leaves the machine:** the hostname (`before_send` clears
  `server_name`), environment variables (blanked out of minidumps, §6.4),
  and credentials: the config keys that hold them are dropped from every
  bundle, crash state and recorded command line (§6.5).
- **Two processes when reporting is on.** Every command, `lsp` included, runs
  as a supervisor and a worker. The supervisor hosts the minidump server,
  keeps what a report needs, and finishes every report through finisher
  children it waits for, so a CI job that ends right after a crash still
  sends it, and nothing is left running detached (§6.2).
- **Only finishers talk to Sentry.** The worker doing the analysis writes
  every crash to disk as a pending report and never opens a connection,
  builds a bundle or starts an async runtime (§6.6).
- **No report is lost to a process ending.** Every report is on disk from
  the moment of the crash until the server accepts it (§6.7).
- **Native crashes: `minidumper` 0.10 and `crash-handler` 0.7 directly.**
  `sentry-minidump` and `minidumper-child` each add a re-executed reporter
  process of their own; the supervisor already outlives the worker, so it
  hosts the minidump server instead.
- **Transport: `sentry` 0.49.3 with our own outbox transport over `reqwest`
  and `rustls`.** Features: `backtrace`, `contexts`, `debug-images`,
  `panic`, `reqwest`, `rustls`, with `default-features = false` to drop the
  default `native-tls` (OpenSSL on Linux). A probe build of exactly this set
  pulls in no `actix-*` crate. `rustls` 0.23 and its `aws-lc-rs` provider
  are already in `Cargo.lock` through the MySQL client, so CI already builds
  them for every target, the release binaries need no `libssl.so`, and the
  aarch64 cross-build needs no arm64 OpenSSL. Certificates are checked
  against the OS trust store (`rustls-platform-verifier`), plus the PEM file
  in `PHPSTAN_RS_SENTRY_CA_FILE` for a server behind a private CA.
- **Crash events only:** no Sentry logs, release health, metrics or
  tracing.
- **Bundles are checked before they're sent**, by replaying them (§6.5),
  and **reduced on the maintainer's side** (§6.13): the bundle already holds
  the full inputs, so reducing on the user's machine would add nothing for
  privacy and cost the user CPU time.
- **Debug info is kept out of the binaries** and uploaded to Sentry by the
  release workflow (§6.11).
- **Two layers, two crates.** Panic isolation is `crates/crash-context`,
  depending on `backtrace` only: the engine code that calls it moves below
  the app in the crate split, and must not depend on Sentry. Reporting lives
  in the app crate's `crash_report` module and plugs in through a `Reporter`
  trait. Without a reporter, isolation still records locations, backtraces
  and focus for internal errors.
- **A broken pipe isn't a crash.** The CLI stops panicking on one (§6.1.5)
  rather than crash reporting learning to ignore it.
- **A `crash-reporting` cargo feature**, on by default, carries all of it.
  Without it, `sentry`, `reqwest`, `minidumper`, `crash-handler` (with its
  `pthread_create` interposer), `crash-sys` and the supervisor's signal
  crates aren't linked. `--no-default-features`, which CI and the gates
  already build, drops it with the rest.

## 3. Measured state

At `56c3beab`:

- **The panic hook is swapped for a silent one** around every isolated
  batch: `main/check.rs:20-33` (`check`), `main/check_project.rs:131-134`
  (`check-project`), `project.rs:1644-1648` (the LSP's
  `check_paths_isolating_panics`) and `composer/src/vendor.rs:64-67` (vendor
  discovery). A panic inside those windows reaches no hook that could report
  it, and those windows are where nearly every caught panic happens.
- **Per-file `catch_unwind`:** `project/session.rs:578` (parse; the panic
  becomes a skipped file, its message dropped, counted in "N file(s)
  skipped: parse error"), `project/session.rs:707` (check),
  `project.rs:1561` (property-assign facts; the file contributes nothing,
  its message dropped), `project.rs:1673` (LSP check), `main/check.rs:54`
  (`check`). A caught panic keeps its message at most: no location, no
  backtrace.
- **The whole-program passes aren't isolated.** `Session::analyse` runs
  `check_whole_program` (dead enum cases, dead functions and classes,
  named-argument facts), `check_mock_hygiene`, `check_class_shapes`,
  `check_banned`, `check_architecture` and `check_composer_hygiene` with no
  `catch_unwind`, inside `check_project.rs`'s silent-hook window. A panic in
  one ends `check-project` with exit code 101 and no message at all.
- **Declaration resolution isn't either.** `Project::finish_loading`
  (`project.rs:703`) runs `canonicalize_class_name_case`, `flatten_traits`,
  `precompute_member_owners`, `resolve_transitive_purity` and
  `precompute_ancestor_names`, most as `par_iter` over every class
  (`registry/resolve.rs`, `hierarchy.rs`, `purity.rs`). A panic there ends
  `check-project` the same silent way. Rayon moves a worker's panic to the
  thread that called into the pool with `resume_unwind`, which runs no hook.
  Only phpstan-rs uses rayon in this workspace (`cargo tree -i rayon`).
- **`ensure_member_index_built`** (`project.rs:830`), which the
  dead-enum-case rule calls when the main pass didn't seed member
  references, re-checks every file in turn with no isolation.
- **Output for a caught panic:** text and friendly print `<path>: internal
  error while checking: <message>` and add "N file(s) errored while
  checking" to the summary; JSON, checkstyle and JUnit drop it; the exit
  code is 0 (every run exits 0 except a config or `--set` error, 1). `check`
  prints `<path>: <message>` for a panic and a parse error alike. The LSP
  logs `phpstan-rs lsp: checking <path> panicked: <message>` on stderr
  (`lsp.rs:642`).
- **The LSP:** `didOpen`, watched-file events, hover, inlay hints, code
  actions and completion probes run on the main or debounce thread, the
  latter with a 2 MiB stack. A panic there unwinds holding the `Server`
  mutex, and the next `lock().unwrap()` panics too. A file with 60 nested
  arrays overflows the debounce thread on `didChange`; 120 abort the server
  during the startup scan. An update (`update_file_cancellable`,
  `project.rs:897`) parses first, then unindexes, reindexes and resolves the
  registry, then checks the affected files. Configuration comes per
  workspace folder from `lsp/workspace_config.rs:68` into a `ConfigFile`,
  and every folder's files go into one `Project`.
- **`process::exit`:** `main.rs:83` (LSP `exit` without `shutdown`),
  `main.rs:91` (`convert-baseline` error), `main/check_project.rs:116,123`
  (config and prepare errors).
- **A broken stdout pipe panics.** Output goes through `println!`, which
  panics when stdout closes: a program printing 200,000 lines into
  `| head -1` exits 101 with "failed printing to stdout: Broken pipe (os
  error 32)" (rustc 1.100.0-nightly, 2026-09-21). `check-project`'s report,
  `check`'s summary, `demo` and `convert-baseline` all print this way.
- **A poisoned SQL cache would cascade.** `SqlContext::reflect`
  (`crates/sql/src/context.rs:61,69`) takes its cache lock with
  `lock().unwrap()`; one panic while it is held would make every later
  SQL-using file fail on the `unwrap`, burying the first panic.
- **Aborts:** a 32,000-term `.` chain aborts `check-project` (Stage D).
  `SIGKILL` (the OOM killer, `kill -9`, a CI runner's hard stop) runs no
  handler anywhere. On Windows, Rust's `abort` (an allocation failure, a
  panic while panicking) uses `__fastfail`, which ends the process without
  running exception handlers; on Linux and macOS it raises `SIGABRT`. This
  machine's `/sys/fs/cgroup` is cgroup v2 (`cgroup2fs`), and its shell's
  cgroup has `memory.events` with an `oom_kill` line.
- **Build:** the x86_64 Linux build links with clang + mold
  (`.cargo/config.toml`), which writes no GNU build ID unless asked; the
  debug binary has none (`readelf -n`). The aarch64 cross-build links with
  Ubuntu's `aarch64-linux-gnu-gcc`, configured with
  `--enable-linker-build-id`. `[profile.release]` has no debug info
  (`lto = "thin"`, `codegen-units = 1`), and `[profile.gate]` inherits it.
- **Release:** five targets (`x86_64`/`aarch64-unknown-linux-gnu` on
  ubuntu-24.04, the aarch64 one cross-compiled;
  `aarch64`/`x86_64-apple-darwin`; `x86_64-pc-windows-msvc`), built with
  `cargo build --release`, gzipped and attached to the GitHub release. The
  VS Code extension downloads them and starts `phpstan-rs lsp` with no
  environment of its own (`extension.ts:67`); its `enable`, `serverPath`
  and `configPath` settings restart the client when changed
  (`extension.ts:133`). Nothing is on crates.io (`publish = false`).
- **Processes that run the binary:** `xtask` (gates, corpus, `hyperfine`
  performance runs, `xtask/src/main.rs:535,602,762,1033`) and the
  integration tests (`tests/cli.rs`, `lsp_exit_code.rs`, `version.rs`)
  spawn `phpstan-rs` with the developer's environment. `tools/gate-runner`
  uses the library in-process. The fuzz targets call `syntax` directly.
- **CI:** `rust-ci.yml` runs on ubuntu-24.04 only. `deny.toml` allows MIT,
  Apache-2.0, ISC and similar licences. `clippy.toml` already bans randomly
  seeded hash types through `disallowed-types`.
- **No cache directory exists yet.** Stage F2 names the user cache
  directory (`$XDG_CACHE_HOME/phpstan-rs` or the platform equivalent) for
  the result cache.

## 4. What the dependencies and the server do

Read from their source; versions as of 2026-09-28.

- **`minidumper` 0.10's server** is public: `Server::with_name(socket)` and
  `run(handler, &shutdown, stale_timeout)`, with a `ServerHandler`:
  `create_minidump_file` (we choose where the dump goes),
  `on_minidump_created`, `on_message(kind, bytes)`, and connect and
  disconnect callbacks. The server process writes the dump by reading the
  crashed process's memory, so a worker that overflowed its stack or ran out
  of memory does no work itself. Messages travel over `SOCK_SEQPACKET` on
  Linux (bounded by the socket buffer) and a stream on macOS and Windows
  read with one `recv` per message (`src/ipc/server.rs:36-70`), so a large
  message can arrive split: messages are for small things only.
- **`sentry-minidump` and `minidumper-child`** re-execute the current binary
  as a crash reporter from inside the app and keep the client side
  (`crash_handler::CrashHandler::attach` with `make_crash_event`) private
  behind that spawn. `make_crash_event` is `unsafe`: its closure runs inside
  the signal handler, and `minidumper-child`'s calls only `client.ping()`
  and `client.request_dump(ctx)`.
- **`crash-handler` 0.7** interposes `pthread_create` on Linux and macOS, so
  every thread gets its own alternate signal stack and a stack overflow on a
  rayon worker is caught. The interposer is linked whenever the crate is,
  and costs one `mmap` and one `sigaltstack` per thread; the signal handlers
  are installed only by `attach`. On Linux it handles `SIGABRT`, `SIGBUS`,
  `SIGFPE`, `SIGILL`, `SIGSEGV` and `SIGTRAP`; on Windows a vectored
  exception handler sees `EXCEPTION_STACK_OVERFLOW`; on macOS it hooks
  `SIGABRT` as well.
- **`minidump-writer` 0.12** copies `/proc/<pid>/environ` and
  `/proc/<pid>/cmdline` into every Linux dump's `LinuxEnviron` and
  `LinuxCmdLine` streams (`src/linux/minidump_writer/mod.rs:422-423`),
  beside `auxv`, `maps`, `status`, `/proc/cpuinfo`, the OS release file,
  the loaded-library list and thread names. Sent as-is, a native crash in
  CI would upload every secret in the job's environment and any `--set` on
  the command line. The writer can't leave streams out. On Windows it
  writes `MiniDumpNormal` (thread stacks, modules, the exception; no heap,
  no environment block); on macOS it has no environment or argument stream.
- **`nix` 0.30** (already a dependency of `minidump-writer`) has safe
  `dup2_stdin` and `dup2_stdout` (`src/unistd.rs:446,492`),
  `set_pdeathsig`, `getppid`, and a kqueue `EVFILT_PROC`/`NOTE_EXIT`
  filter; it has no `setpriority`.
- **Linux runs a deleted binary through `/proc/self/exe`.** Once the
  running binary's file is deleted (an upgrade replacing it),
  `std::env::current_exe` returns `"<path> (deleted)"` and spawning it
  fails with `NotFound`, while spawning `/proc/self/exe` still runs the
  same program (checked with a scratch binary deleted while running).
- **Sentry SDK:** `sentry-backtrace` turns a `backtrace::Backtrace` into a
  stacktrace (`backtrace_to_stacktrace`); `sentry-panic` exposes
  `PanicIntegration::event_from_panic_info`; envelopes serialise with
  `Envelope::to_writer` and load with `Envelope::from_path`;
  `sentry::transports::RateLimiter` parses `X-Sentry-Rate-Limits` and
  `Retry-After`; a custom transport implements `sentry::Transport`.
  `Hub::current()` on a thread other than the main one is a snapshot of the
  main hub taken at first use, so later breadcrumbs don't reach it.
  Defaults: `shutdown_timeout` 2 s, `max_breadcrumbs` 100.
- **Relay** (`relay-config/src/config.rs`): 1 MiB per event, 200 MiB per
  attachment, for all attachments together and per envelope; self-hosted's
  nginx caps every request body at 100 MB (`client_max_body_size 100m`).
  Relay rejects no event for its age: a timestamp older than the project's
  event retention (`DEFAULT_EVENT_RETENTION`, 90 days) is replaced by the
  arrival time, with a `past_timestamp` error
  (`relay-event-normalization/src/validation.rs`).
- **Sentry:**
  - `sentry:store_crash_reports` defaults to 0 (`lang/native/utils.py`):
    minidumps are symbolicated, then dropped; -1 keeps all, a number keeps
    that many per issue, at most 100. Other attachments are stored
    regardless (`event_manager.py`).
  - Downloading an attachment
    (`api/endpoints/event_attachment_details.py`) needs an authenticated
    *user* whose organization role reaches `sentry:attachments_role`
    (default `member`); an organization auth token has no user and is
    refused. `?download` returns the file, possibly as a redirect to the
    object store, possibly content-encoded as stored.
  - Organization auth tokens carry `org:ci`, which
    `ProjectReleasePermission` accepts for debug-file uploads and releases
    (`api/bases/project.py`).
  - Self-hosted sets `SENTRY_QUOTAS = "sentry.quotas.redis.RedisQuota"`
    (`sentry.conf.example.py`), so client-key rate limits are enforced.
- **`sentry-cli` 3.8.0** has `debug-files upload --include-sources`,
  `debug-files check`, and `releases new`, `set-commits --local` and
  `finalize`; `--auto` would need a repository integration configured in
  Sentry. `sentry-cli-Linux-x86_64` from the 3.8.0 GitHub release has
  SHA-256
  `13f8cb34ae01a6a272d7d7c22e277a105286615b4020de900ea95a8de47cdbb6`; the
  release publishes no checksum file.
- **Licences:** every crate the probe build adds passes `deny.toml`
  (`aws-lc-sys`'s compound expression is already in the lock); `win32job`,
  `signal-hook`, `rcgen`, `dirs` and `tar` are MIT or MIT OR Apache-2.0.

## 5. Layout

- **`crates/crash-context`** (`crash_context`): the hook, `isolated`,
  `carry`, `entry`, `focus`, panic records, `InternalError`, the worker's
  breadcrumb, tag and context store, and the `Reporter` trait. Depends on
  `backtrace` only. The engine crates depend on it.
- **`crates/crash-sys`**: `attach`, `release_std_handles`,
  `enter_background` (§6.2.3). The only crate allowed `unsafe`.
- **`crates/phpstan-rs/src/crash_report/`**: `init`, `supervisor`,
  `pending`, `bundle`, `finish`, `outbox`, `state`, `lsp`; implements
  `Reporter`. The app crate re-exports `isolated` and the rest for its own
  call sites.
- **`tools/crash-repro`**: `replay`, `reduce`, `fetch`, using `bundle`
  through the library.
- The hook installs itself on first use: `isolated`, `entry` and `focus`
  each call `crash_context::install_hook()`, a `Once`, so every user of the
  library (the binary, `tools/gate-runner`, the unit tests, `crash-repro`)
  gets locations and backtraces in its internal errors; `main` calls it
  first. The previous hook it keeps is std's, or libtest's capturing one in
  tests, so test output capture still works. Functions documented as
  panic-transparent (`Project::check_paths`, the fuzz targets' entry
  points) never pass through `isolated`.

## 6. Design

### 6.1 Panic isolation

This part holds with or without a DSN; it is step 1.

#### 6.1.1 The hook

One hook, installed once, replaces the four swaps. Three thread-locals drive
it: `isolated` (inside an isolated unit), `focus` (§6.8) and `reporting`
(inside crash-report code). A panic record holds the message, the location,
the thread's focus at that moment and an unresolved `backtrace::Backtrace`,
captured when a DSN is set or `RUST_BACKTRACE` is, and resolved only when
printed or written into a report. The hook:

- **with `reporting` set** prints the default message and does nothing
  else;
- **on an isolated thread** prints nothing and stores the record in a
  thread-local, which `isolated` takes into the unit's internal error;
- **inside a parallel closure** does the same (below);
- **anywhere else** prints the default message (the previous hook) and,
  with a DSN, calls the `Reporter`'s `unisolated(record)`, which builds the
  event (`PanicIntegration::event_from_panic_info`, the backtrace resolved
  here, where its addresses are valid) and writes a pending report (§6.4).
  A location already reported by this process is counted, not reported
  again, and a process makes at most 3 pending reports, so a process
  panicking in a loop can't flood its supervisor.

The hook never panics: every step returns a `Result` it ignores.

**Parallel closures.** Rayon carries a worker's panic to the calling thread
with `resume_unwind`, which runs no hook. Every `par_iter`, `into_par_iter`
and `par_chunks` closure (the eight in `registry/resolve.rs`,
`hierarchy.rs`, `purity.rs` and `stubs.rs`, and the per-file ones in
`project.rs`, `session.rs` and `main/check.rs`) runs its body through
`crash_context::carry(|| ..)`: it marks the thread, catches a panic on the
worker, takes the record, and resumes with a `CarriedPanic(record)` payload
that `isolated` and `entry` unwrap. `clippy.toml` gains
`disallowed-methods` entries for rayon's constructors (`par_iter`,
`into_par_iter`, `par_chunks`, `par_bridge`, `join`, `scope`) with the
reason "run the body through `crash_context::carry`"; each audited site
carries `#[allow(clippy::disallowed_methods)]`, so a new site can't skip
it.

**Thread tops.** `crash_context::entry(|| ..)` wraps `main`'s body and
every thread the program spawns (the LSP's reader and debounce threads). A
`CarriedPanic` escaping to it is printed (the hook held the message back),
reported through `unisolated`, and resumed, so the process or thread ends
as it does today.

Vendor discovery's swap is deleted rather than converted: its parse already
returns errors (B8, "No global panic hook"). `composer` gains no
dependency.

#### 6.1.2 Internal errors

`isolated(unit, || ..)` runs the unit under `catch_unwind` and turns a
panic into:

```rust
pub struct InternalError {
    pub unit: Unit,                 // Parse(path) | Facts(path) | Check(path) | Pass(name) | Load(step)
    pub focus: Option<PathBuf>,     // the panicking thread's focus
    pub message: String,
    pub location: Option<Location>, // file:line:col in phpstan-rs's source
    pub backtrace: Option<backtrace::Backtrace>,
}
```

It also hands the error to the `Reporter`. The parse site stops dropping a
panic into a skipped file and the facts site stops dropping it into an
empty contribution: both record an internal error.
`AnalysisResult::check_panics` becomes `internal_errors:
Vec<InternalError>`, sorted by unit.

#### 6.1.3 Isolated units

- **Per file:** parse, property-assign facts and check, in `check-project`,
  `check` and the LSP.
- **Whole-program passes:** `Session::analyse` runs `dead_enum_cases`,
  `dead_code`, `named_arguments`, `mock_hygiene`, `class_shapes`, `banned`,
  `architecture` and `composer_hygiene` (per root) each as
  `isolated(Unit::Pass(name), ..)`; `check_whole_program`'s three rule
  families become three passes. A pass that panics contributes no
  diagnostics; the others still run. Each pass sets the focus as it moves
  between declarations.
- **`ensure_member_index_built`:** each file's re-check is its own
  `Unit::Check(path)`; a file that panics contributes no member references.
- **Load:** `finish_loading`'s five steps (`canonicalize_class_names`,
  `flatten_traits`, `member_owners`, `transitive_purity`,
  `ancestor_names`) each run as `isolated(Unit::Load(step), ..)`, their
  `par_iter` closures setting the focus to the declaring file of the class
  they're on. A failed step leaves the registry unusable, so the run ends
  with that internal error rather than analysing half-resolved
  declarations.

#### 6.1.4 Output

A behaviour commit per format, each with its `tests/cli.rs` changes:

- **Text and friendly:** one line per internal error, `<path>: internal
  error while parsing|collecting property facts|checking: <message> (at
  <location>)`, or `internal error in <pass>: <message> (at <location>)`
  plus `(while on <focus>)`; with `RUST_BACKTRACE` set, the resolved
  backtrace follows, indented. The summary says "N internal error(s)" and
  "the result is incomplete" in place of "N file(s) errored while
  checking", and a parse panic no longer counts as skipped. A load step's
  error ends the run, printed as `internal error while <step>: <message>
  (at <location>)` plus the focus, the way `Session::run` errors print
  today.
- **`check`:** `<path>: internal error: <message> (at <location>)` for a
  panic; `<path>: <message>` stays for a parse error.
- **JSON:** a top-level `internal_errors` array of `{ "path", "unit",
  "message", "location", "focus" }` (path and focus may be null),
  `totals.internal_errors`, and `"incomplete": true` when any exist.
  `totals.files_checked` keeps excluding every file with an internal error,
  now including parse panics, so its value doesn't change.
- **Checkstyle:** `<error severity="error" source="phpstan-rs.internal"
  message="internal error while <unit>: <message> (at <location>)"/>`
  without a line, under the file's `<file>`; a pass or load error goes
  under its focus file, or the config file (the working directory without
  one).
- **JUnit:** one failing `<testcase classname="phpstan-rs.internal"
  name="<unit> <path>">` per internal error, its `<failure>` holding the
  message and location.
- **The LSP's log line:** `phpstan-rs lsp: internal error while checking
  <path>: <message> (at <location>)`.
- **Exit code** (F1's item, whole): 0 when nothing remains after the
  post-filters; 1 when a file or not-file error remains; 2 when the run
  couldn't start (an invalid config, no files under the paths, an
  unreadable baseline) or finished incomplete (an internal error).

#### 6.1.5 Broken pipes and poisoned caches

- **Broken pipes:** the CLI's output (`check-project`'s report in every
  format, `check`'s summary, `demo`, `convert-baseline`) is written through
  a locked `stdout` with `write!`, errors propagated. A `BrokenPipe` ends
  the run quietly with exit code 141 (128 + `SIGPIPE`, what a shell reports
  for a program the signal ended); any other write error is printed and
  exits 1. This is F3's formatters writing to a `Write`, for the formats
  that exist today. Stderr keeps `eprintln!`: a closed stderr has no one to
  tell.
- **The SQL cache:** `SqlContext::reflect` takes its lock with
  `lock().unwrap_or_else(PoisonError::into_inner)`. Its critical sections
  are a `get` and an `insert`, which leave the map valid whatever panicked
  around them.

#### 6.1.6 The LSP

- **Handlers on the pool** (F1's "Panic isolation, on the pool"): the
  workspace scan, `didOpen`, the debounced `didChange` recheck, watched-file
  events, folder removal, and the requests that parse or check (hover,
  inlay hints, code actions, completion probes, references' first member
  index) run on `project::worker_pool`'s 64 MiB stacks, so the files that
  abort the server today are checked there as `check-project` checks them.
- **Isolated where nothing is mutated:** an update's parse and check phases
  run as isolated units. A panic there is the file's internal error; a
  request that hit one answers a `ResponseError` (`InternalError`,
  -32603); the server carries on with consistent state, and its `Server`
  mutex is never poisoned.
- **The registry phase stays unisolated.** A panic halfway through
  unindexing, reindexing and resolving leaves declarations half-updated,
  and the only proper recovery is C4's snapshots, which let Stage F discard
  the update and keep the last good state. Until then that panic stays a
  crash, reported through `entry`, and the editor restarts the server. The
  `Server` mutex keeps `unwrap` for the same reason.
- **Focus and breadcrumbs:** `run_dispatch_loop` sets the focus and adds a
  breadcrumb per message before `handle_request`, `handle_notification` or
  `queue_change`; the debounce worker does the same per recheck.

### 6.2 Processes

#### 6.2.1 Roles

Without a DSN a run is one process, as today. With one:

| Role | Started by | Does |
| --- | --- | --- |
| supervisor | the user, CI or the editor | hosts the minidump server, starts the worker, keeps the crash state, finishes reports, relays the exit status |
| worker | the supervisor | the run, with the crash handler attached; writes pending reports, never sends |
| finisher | a supervisor, only as its child | completes pending reports with their bundles and sends them (§6.7) |

`crash_report::init()` is the first thing `main` does, before
`Cli::parse`, the dhat profiler and anything else, because a supervisor
must start its worker before any app work. With `PHPSTAN_RS_SENTRY_DSN`
unset it returns and nothing else in this plan runs. With it set,
`_PHPSTAN_RS_CRASH_ROLE` (`worker` or `finisher`) gives a child its role; a
process without one is a supervisor. The supervisor has no Sentry client
and no network code.

`init` failing leaves the run without reporting, with one line on stderr:
an invalid DSN, or no usable cache directory (§6.3). In a `hotpath` build
the `#[hotpath::main]` wrapper runs before `init`, so a supervisor or
finisher starts its own hotpath guard; profiling builds are never run with
a DSN.

#### 6.2.2 The socket

The supervisor creates `minidumper::Server::with_name` on a fresh socket and
runs it on a thread with our `ServerHandler`, then starts the worker with
the name in `_PHPSTAN_RS_CRASH_SOCKET`. The worker connects a
`minidumper::Client` and calls `crash_sys::attach(client)`. The worker's
messages, each under 4 KiB:

- `FOCUS` and `FOCUS_CLEAR`, a thread name and a unit's path (§6.8);
- `BREADCRUMB` and `TAG`, mirrored so a native crash carries them;
- `PENDING`, a pending report's id, once the worker has written it.

Anything larger goes through the crash-state directory (§6.3).

**Only the user can reach it.** The socket is a path inside a `0700`
directory, never a Linux abstract name: an abstract socket has no
permissions, so any local process could connect, send messages and ask the
supervisor to dump a process. The directory is
`$XDG_RUNTIME_DIR/phpstan-rs` when set, otherwise the run's crash-state
directory; when that path is longer than a socket address holds (108 bytes
on Linux and Windows, 104 on macOS), `std::env::temp_dir()/phpstan-rs-<uid>`,
created `0700` and refused if someone else owns it. The name is
`<run id>.sock`, removed when the supervisor exits. On Linux `attach` also
restricts reading the worker's memory to the supervisor (`set_ptracer`).

#### 6.2.3 `crash-sys`

`#[allow(unsafe_code)]` on three functions only (the workspace lint is
`deny`, not `forbid`):

- `attach(client: Arc<minidumper::Client>) -> Result<CrashHandler, Error>`
  wraps `CrashHandler::attach(unsafe { make_crash_event(..) })`. SAFETY:
  the closure runs in a signal handler and calls only
  `client.request_dump(ctx)`, which `minidumper` provides for that context,
  as `minidumper-child`'s closure does; it allocates nothing and takes no
  lock. On Linux it then calls `set_ptracer(Some(supervisor_pid))`.
- `release_std_handles()` closes a Windows process's own standard input and
  output handles (`GetStdHandle`, `CloseHandle`, `SetStdHandle` to null),
  treating a missing or invalid handle (a service, a detached process) as
  already released. On Unix the supervisor does the same safely with
  `nix::unistd::dup2_stdin`/`dup2_stdout` onto `/dev/null`.
- `enter_background()` makes the calling process background work: Unix
  `setpriority(PRIO_PROCESS, 0, 19)`, Linux also `ioprio_set` to the idle
  class, macOS also `setiopolicy_np` with `IOPOL_THROTTLE`, Windows
  `SetPriorityClass` with `PROCESS_MODE_BACKGROUND_BEGIN` (CPU, I/O and
  memory priority together).

#### 6.2.4 The supervisor

1. Creates the crash-state directory (§6.3) and the minidump server.
2. Starts the worker: the running binary (below), the same arguments
   through `std::env::args_os` so non-UTF-8 ones pass exactly, inherited
   stdio, and the role, socket, run id, crash-state path and its own pid in
   the environment.
3. Releases its own standard input and output, so the worker holds the only
   copies: when the worker ends, a reader of its stdout (an editor's
   language client, a pipe) sees the end at once, not when the supervisor
   finishes its reports. Stderr stays, for the supervisor's own lines.
4. Starts a draining finisher when earlier runs left anything in the outbox
   (§6.7).
5. Waits for the worker, collecting `PENDING` messages. For `lsp` it starts
   a finisher per report as it arrives, one at a time; for any other
   command, one finisher for all of the run's reports once the worker has
   ended, so the project is loaded once.
6. When the worker has ended abnormally:
   - with a dump written, completes that report (§6.4);
   - with no dump (on Windows a `__fastfail` abort, exit code
     `0xC0000409`; anywhere, a crash the handler missed), writes a report
     itself, `unit = native`, "worker terminated by `<signal or exception
     code>`", with the crash state, the threads' last `FOCUS` as the
     in-progress files, and the mirrored breadcrumbs and tags;
   - after a `SIGKILL`, reports only an OOM kill. On Linux it reads its
     cgroup's `memory.events` (cgroup v2: the `0::` line of
     `/proc/self/cgroup`, under `/sys/fs/cgroup`) when it starts the worker
     and when the worker ends; the kernel counts in `oom_kill` every process
     of the cgroup killed by any OOM killer, the system-wide one included.
     A `SIGKILL` while `oom_kill` rose becomes a report with `unit = oom`,
     the in-progress files, breadcrumbs and crash state, and the cgroup's
     `memory.max` and `memory.peak`. Another process of the same cgroup
     OOM-killed at the same moment could be mistaken for the worker; the
     worker's own `SIGKILL` in that window makes that unlikely. Every other
     termination from outside (`SIGKILL` without an OOM kill, `SIGTERM`,
     `SIGINT`, `SIGHUP`) is a user's or a CI runner's decision and isn't
     reported; without cgroup v2 (macOS, Windows, cgroup v1) no `SIGKILL`
     is.
7. Waits for its finishers, up to 60 s in all, printing `phpstan-rs:
   sending the crash report to <host>` once it waits on one, and
   `phpstan-rs: N crash report(s) kept in <outbox> for the next run` for
   any a finisher couldn't send. A `SIGINT` or `SIGTERM` during the wait
   ends it at once; what is unfinished stays for the next supervisor (in
   the LSP, the one the editor starts next).
8. Deletes the crash-state directory and exits with the worker's code, or
   after a crash with 128 + the signal number on Unix and the worker's
   exception code on Windows.

**The running binary.** On Linux the supervisor starts its worker and
finishers by executing `/proc/self/exe`, so an upgrade that replaces the
installed file (a package manager, the VS Code extension's download)
doesn't stop a language server running for days from starting finishers of
its own version (§4). macOS and Windows have no equivalent: there
`current_exe` is used, and a failed spawn degrades (below).

**The worker never outlives the supervisor.** On Unix the supervisor
forwards `SIGTERM`, `SIGHUP` and `SIGINT` to it (`signal-hook`'s iterator
and `nix::sys::signal::kill`), so a CI timeout or an editor stopping the
server stops the work. On Linux the worker sets `PR_SET_PDEATHSIG` to
`SIGTERM`, then checks that `getppid` is still the supervisor's pid and
exits if not, since the supervisor may have died first. On macOS a worker
thread waits on a kqueue `EVFILT_PROC`/`NOTE_EXIT` event for the
supervisor's pid after the same check. On Windows the worker runs in a job
object with kill-on-close (`win32job`).

**The supervisor's own failures never end the worker.** Everything but
waiting, forwarding signals and relaying the exit status (the server thread
and its callbacks, writing reports, starting finishers) runs under
`catch_unwind`, and a failure there costs reporting, not the run. A dead
server thread leaves the worker's crash handler unable to request a dump,
so a later native crash is reported through the no-dump path.

**Degraded starts.** A supervisor that can't start its worker or create its
socket runs the command itself as a worker with no crash handler, with one
line on stderr. A worker that can't connect or attach runs without
native-crash reports. A finisher that can't be started leaves its reports
for the next supervisor, whose finisher may be a newer version (§6.7).

**Containers and services:**

- **PID 1:** in `docker run <image> phpstan-rs ...` the supervisor is
  PID 1, which the kernel gives no default signal actions; it handles and
  forwards them, so `docker stop` stops the run. As PID 1 it also reaps
  every child it didn't start (`waitpid(-1)` in its wait loop), so orphans
  leave no zombies.
- **Process limits:** a cgroup's `pids.max` counts threads; the worker runs
  one per CPU, and the supervisor, a finisher and a replay add three
  processes. A spawn refused for the limit degrades as above.
- **`ptrace` refused** (gVisor, a strict seccomp profile): the dump fails,
  and the supervisor reports through the no-dump path.

### 6.3 Crash state

- **The cache directory:** `crash_report::cache_dir()` is
  `dirs::cache_dir()` + `phpstan-rs`, created `0700` on Unix since it holds
  source. Without a user cache directory (no `HOME` in a container) or when
  it can't be written, `std::env::temp_dir()/phpstan-rs-<uid>` (the user's
  name on Windows), created `0700` and refused if someone else owns it;
  when that fails too, reporting is off for the run, with one line on
  stderr, since every report passes through the disk.
  `_PHPSTAN_RS_CACHE_DIR` overrides it for tests running in parallel. F2's
  result cache reuses the function.
- **The run's directory:** the supervisor creates
  `<cache>/crash-state/<run id>/`, holds `supervisor.lock` in it for its
  whole life, passes its path in `_PHPSTAN_RS_CRASH_STATE`, and deletes it
  on exit; a directory whose `supervisor.lock` can be taken belongs to a
  supervisor killed outright, and the next supervisor removes it.
- **What the worker keeps there:** `state.json`, with a format version, the
  command line, working directory, project root, config path and effective
  config table (secret keys dropped, §6.5); in the LSP also the workspace
  folders, each folder's config path and table, the client's
  `initializationOptions` and `PhpstanRsSettings`, and `clientInfo`,
  rewritten when they change. The LSP writes an overlay into `overlays/`
  when `didOpen` brings text that differs from the file on disk (an editor
  restoring an unsaved buffer) and whenever the debounce worker takes a
  change, and removes it on `didSave` and `didClose`.
- **Locks:** every lock in this plan is a dedicated empty `*.lock` file
  taken with `File::lock`/`try_lock`, never the data it guards: Windows
  locks are mandatory, so locking a data file would stop other processes
  reading it.

### 6.4 Pending reports

Every crash becomes a pending report the moment it happens, and every
report is finished the same way (§6.7). A pending report is
`<cache>/crash-outbox/pending/<id>/`:

```text
report.json   a panic: unit, message, location, focus, resolved frames,
              module list, tags, context, breadcrumbs, binary identity
minidump.dmp  a native crash: the scrubbed dump
plan.json     a panic: the bundle's file walk and buffer texts
state/        the crash state as the crash saw it, and the in-progress files
meta.json     format version, DSN hash, created, attempts, run id, unit
entry.lock    held by the worker while writing, then by the finisher
finisher.log  the finisher's stderr
```

- **Panics, isolated or not:** the worker writes the report there and then,
  with `reporting` set and under `catch_unwind`, and sends `PENDING`. It
  writes `state/` itself (`state.json` and every open buffer's text as they
  were), since a copy the supervisor made after the message could already
  hold a later edit. It resolves the backtrace here, where the addresses
  are valid, and records its module list for symbolication. A location
  this process already wrote is added to that report's file list instead. A
  run interrupted right after keeps the report on disk.
- **The plan:** for a panic the worker also writes `plan.json`, the walk of
  §6.5 run against the live registry: every file up to the cap in walk
  order, with its distance, role, size and content hash, and the text of
  every open buffer. With it the finisher only reads the listed files and
  never loads the project, so a finisher beside a language server costs the
  files' size, not a second workspace.
- **Native crashes:** the supervisor's `create_minidump_file` creates the
  report and returns `minidump.dmp` inside it. `on_minidump_created` first
  blanks the data of the `LinuxEnviron` (`0x47670007`) and `LinuxCmdLine`
  (`0x47670006`) streams in place (the header's stream count and directory
  offset, then each 12-byte directory entry's type, size and offset; no
  parser crate) before anything reads the file. Then it copies the crash
  state, the threads' last `FOCUS` and the mirrored breadcrumbs and tags
  in, and writes `meta.json` with `unit = native`. What a dump still holds
  is each thread's stack memory, which can include fragments of what the
  program was handling (source text; during a live SQL connection, its
  credentials); the README says so.
- **Abnormal ends without a dump and OOM kills:** the supervisor writes the
  report (§6.2.4, step 6).
- **Disk full:** a report that can't be written is lost, with `phpstan-rs:
  couldn't keep a crash report: <error>` on stderr; the run carries on.

### 6.5 Bundles

One `repro.tar.gz` attachment per event, built by `bundle::build`:

```text
manifest.json
phpstan-rs.toml          the effective config, rewritten
project/<path>           files at their paths under the project root
external/<n>/<name>      files outside the root, mapped in the manifest
```

- **`manifest.json`:** the release and commit, the command line, the crash
  (unit, full message, location or signal, focus), the file list of every
  crash grouped into this report, each file with its role (`crashing`,
  `dependency`, `dependent`, `stub`, `composer`), size, SHA-256 and whether
  it came from an unsaved buffer or `changed_since_crash`, the files left
  out for the cap, and for the LSP the workspace folders.
- **Files, per unit:**
  - `parse`, and `check`'s run: the file alone;
  - `facts` and `check`: the file, then breadth-first by reference distance
    the files declaring what it references (each name in its
    `FileState::references`, through the registry's `Provenance::file`), a
    class's parents, interfaces and traits always one level ahead of
    anything else at the same distance;
  - `pass:<name>` and `load:<step>`: the focus file, then the same walk
    both ways, through references and `Project::dependents` (whole-program
    passes read the files referencing a declaration); with no focus, the
    manifest alone, tagged `no_focus`;
  - `native` and `oom`: every in-progress file (the dump doesn't say which
    thread caused it, and an OOM kill leaves none), each walked as `check`.

  Then the user's stub files (`stub_paths`), and `composer.json` and
  `composer.lock` from each discovered root (they set the PHP version
  range). The walk fills the bundle up to the cap rather than stopping at a
  depth.
- **Cap:** 10 MiB compressed or 64 MiB uncompressed, whichever comes first,
  well under the 100 MB a self-hosted instance accepts, since a CLI run
  waits for the upload; 10 MiB of gzipped PHP is about 60 MiB of source. A
  crashing file larger than the cap alone is sent as the manifest only,
  tagged `too_large`.
- **Config:** the effective `ConfigFile` table (old keys moved, `--set`
  applied), serialised back to TOML. `config::Schema` gains two key
  registries, filled by the sections that own the keys:
  - **path keys**, rewritten: `paths` becomes the crashing files;
    `scan_paths` the dependency and dependent files; `stub_paths` the
    bundled stubs; `ext.sql.reflection.schema_file` the bundled DDL file;
    `baseline` and `ignore_errors` are dropped, since they act after the
    check;
  - **secret keys**, dropped everywhere (bundles, crash state, recorded
    command lines): `ext.sql.reflection`'s `host`, `port`, `user`,
    `password`, `password_env`, `database`, `path` (a SQLite database file)
    and `session_init_sql`. A `--set` on one is recorded as
    `--set <key>=<redacted>`.

  A schema test fails when a section deserialises a `PathBuf` field whose
  key is in neither registry. In the LSP, the folder's config as
  `workspace_config::resolve` loaded it.
- **Live SQL reflection:** the reflected catalog is written as DDL
  (`SchemaCatalog::to_ddl`, new, the inverse of `parse_ddl` for the tables,
  columns and types the catalog holds) into `live-schema.sql`, which the
  bundle's config reads as its `schema_file`. What a live connection adds
  beyond the catalog (whether the server accepts a query at prepare time)
  isn't in the bundle; the manifest says `sql: live`.
- **Contents as analysed:** a file comes from the plan's recorded text or
  the project's retained source when there is one; otherwise from disk, and
  a file whose content hash no longer matches the plan, or whose
  modification time is later than the run's start, is marked
  `changed_since_crash`.
- **Paths** use `/` in the archive on every platform; a path that isn't
  UTF-8 is exact in the tar header and lossy, with a flag, in the manifest.
  The archive is `tar` + `flate2` (`flate2` is already in the lock).
- **Checked before sending.** The finisher replays the bundle in a child,
  `phpstan-rs __crash-replay <bundle>` (hidden like `__crash-finish`): its
  files loaded into a fresh project with its config, the crashing unit run
  under `isolated`, the result compared with the crash (the same panic
  location; for a native crash, the child dying of the same signal). A
  bundle that doesn't reproduce is widened (the plan's next distance; for a
  pass or load unit, every analysed file) and replayed again, at most three
  times within the cap. The event is tagged `reproduces` = `yes`, `no`,
  `not_in_replayed_with` (§6.7) or `unchecked` (a live SQL connection the
  bundle can't carry, a replay that timed out after 60 s). A replay runs
  only phpstan-rs's analysis over the files, never the project's code.
- **Across runs:** `<cache>/crash-reports/sent.json` records a bundle key,
  `(release, panic location, crashing file's content hash)` for a panic and
  `(release, signal or exception code, in-progress files' hashes)` for a
  native crash or OOM kill, for each bundle the server accepted. A repeat
  within 30 days still sends the event, so Sentry's counts stay true, but
  tagged `bundle = already_sent`, without the bundle. It is updated under
  its lock and replaced by rename, and carries a format version.

### 6.6 Where Sentry runs

Only finishers call `sentry::init`, with:

- `dsn` from the variable; `release` = `phpstan-rs@<version>+<commit>`
  (`PHPSTAN_RS_BUILD_COMMIT` at compile time through `option_env!`, set by
  the release workflow; `+dev` without it); `environment` = `release` for a
  `dist` build (`PHPSTAN_RS_DIST` at compile time), `development`
  otherwise. A build script reading `git rev-parse HEAD` would force the
  crate and everything above it to recompile on every commit; instead each
  event carries the binary's build ID and its executable's size and
  modification time as the extra `binary`, recorded by the worker, which
  tell two development builds apart;
- `default_integrations = false`, then the contexts integration (the
  machine, which is the worker's too) and debug images taken from the
  worker's recorded module list, since only the worker's load addresses
  match its frames;
- `send_default_pii = false`; `before_send` clears `server_name`;
- `in_app_include`: the workspace crates (`phpstan_rs`, `syntax`, `types`,
  `phpdoc`, `sql`, `diagnostics`, `composer`, `config`, `stub_sources`,
  `crash_context`);
- the outbox transport (§6.7) and `shutdown_timeout` 50 s, inside the
  supervisor's 60 s wait;
- `aws-lc-rs` installed as the process's rustls provider explicitly, so a
  second provider entering the graph can't make rustls panic for want of a
  default.

The worker's side of reporting is `crash_context`'s own store, not a Sentry
hub: breadcrumbs (the last 100), tags and context in process-wide storage
every thread writes to, so a report written from any thread carries the
whole run's, each mirrored to the supervisor (`BREADCRUMB`, `TAG`) for a
native crash. Every `process::exit` in the worker (`main.rs:83`,
`main.rs:91`, `main/check_project.rs:116,123`) goes through
`crash_report::exit(code)`, which makes sure pending reports are written
first.

### 6.7 Finishing and sending

**The finisher**, `phpstan-rs __crash-finish <report>...` (hidden from
`--help`), always a supervisor's child, with null stdin and stdout and its
stderr in `finisher.log`:

1. calls `crash_sys::enter_background()`, so building a bundle beside a
   live language server or at the end of a CI job never competes with
   analysis;
2. takes each report's `entry.lock` with `try_lock`, skipping any another
   finisher holds, and increments its attempt count on disk before anything
   that could crash;
3. builds the event: from `report.json` for a panic, one per location, with
   the message cut at 8 KiB on a character boundary (the full message goes
   into the manifest), the fingerprint `["panic", <location>]` (stable
   across symbolicated and unsymbolicated builds), and the file count and
   first 20 paths as extras (Sentry trims large extras); for a native crash
   or an OOM kill, a fatal event with `meta.json`'s breadcrumbs and tags,
   and the dump as an `event.minidump` attachment from which the server
   derives the crash itself;
4. checks `sent.json`: a bundle sent within 30 days isn't built again;
5. on the first two attempts, when the crash state's format version is its
   own, builds the bundle for the report's smallest crashing file and
   replays it (§6.5): from `plan.json` for a panic; for a native crash or
   an OOM kill, which leave no live registry, after loading the project
   from `state/` (parse and index only: `Session` from `state.json` for
   `check-project`; for `lsp`, `lsp::load_workspace(folders, settings,
   overlays) -> Project`, the load `Server::add_workspace_folders` does
   today moved into a function both call; nothing for `check`);
6. on a third attempt, or when loading or building fails, attaches the
   crashing files and the effective config directly, tagged
   `bundle = failed`, with the failure as an extra;
7. moves the finished envelope into `ready/` and sends it.

A finisher that crashes leaves its reports for the next attempt, and the
attempt count keeps a project that crashes the loader from looping forever;
a finisher runs without a crash handler of its own. `--drain` makes it also
send every `ready/` entry and finish every free `pending/` report.

**A finisher of another version.** A report left behind can be finished by
a later supervisor's newer binary. Everything describing the crash comes
from the report (the release, the binary's identity, the frames and module
list), so the event names the version that crashed. The replay runs with
the finisher's version: the event carries `replayed_with`, and when that
release differs and the replay doesn't reproduce, `reproduces =
not_in_replayed_with`, which says the newer version may have fixed it
rather than that the bundle is incomplete.

**The outbox:**

```text
<cache>/crash-outbox/ready/<id>/
  envelope      the finished, serialised envelope
  meta.json     format version, DSN hash, created, attempts, not_before, bundle key
  entry.lock    held by whoever is writing or sending it
<cache>/crash-outbox/pending/<id>/   (§6.4)
```

- **The transport** writes each event envelope into `ready/` before
  sending; SDK-internal envelopes (client reports) are sent unkept. It
  POSTs with its own `reqwest` client (the platform verifier plus
  `PHPSTAN_RS_SENTRY_CA_FILE`, an unreadable one leaving the system roots
  with one line on stderr; `HTTPS_PROXY`/`NO_PROXY` honoured as `reqwest`
  does; a 10 s connect and a 120 s request timeout, enough for 10 MiB on a
  slow link) and applies `RateLimiter`. A 2xx deletes the entry and, for an
  event with a `bundle_key` extra, records the key in `sent.json`, so a
  bundle counts as sent only once the server has it. A network error or a
  5xx keeps the entry; a 429 keeps it with `not_before` from the limit; any
  other 4xx deletes it with `phpstan-rs: the crash-report server rejected a
  report (<status>)` on stderr. When `ready/` can't be written, the
  envelope is sent directly, unkept.
- **Senders never collide:** a sender takes `entry.lock` with `try_lock`
  and skips an entry it can't, so two runs draining at once never send a
  report twice.
- **DSN and versions:** an entry is sent only by a client with the same DSN
  hash, so switching servers never sends old reports to the new one. An
  entry with a newer format version than the running binary's is left
  alone (a downgrade); an older one is read by its version's rules, which
  each format change keeps.
- **Draining:** at start, the supervisor removes crash-state directories
  whose `supervisor.lock` is free and, when `ready/` or `pending/` holds
  anything, starts one `--drain` finisher while the worker runs.
- **Expiry:** entries older than 30 days are deleted, and the oldest go
  first once the outbox passes 100 MiB. Within 30 days a retried event
  keeps its crash's time, since Relay only rewrites timestamps older than
  the project's retention.

### 6.8 Tags, context, breadcrumbs, focus

- **Tags:** `command` (`check`, `check-project`, `lsp`), `level`, `unit`
  (`parse`, `facts`, `check`, `pass:<name>`, `load:<step>`, `native`,
  `oom`), `pcre_validate`, `lsp.client` and `lsp.client_version` (from
  `initialize`'s `clientInfo`, which is also written into the crash state),
  `bundle` (`attached`, `already_sent`, `too_large`, `no_focus`, `failed`),
  `reproduces`, `replayed_with`, `ci`, `ci.provider`.
- **Context `run`:** analysed and context file counts, the enabled optional
  rules, the PHP version range, whether ext-sql reflects a live database;
  for an OOM kill, the cgroup's `memory.max` and `memory.peak`.
- **Installation:** `user.id` is a random id made once and kept in
  `<cache>/installation-id`, so Sentry's "users affected" counts machines
  without identifying anyone; deleting the file makes a new one. A CI
  machine is fresh each run, so CI is tagged instead: `ci = true` when `CI`
  is set, and `ci.provider` from the variables providers set
  (`GITHUB_ACTIONS`, `GITLAB_CI`, `BUILDKITE`, `CIRCLECI`, `JENKINS_URL`,
  `TF_BUILD`), reading only their presence, never their values.
- **Breadcrumbs:** each run phase (config, discovery, load, property-assign
  facts, check, each whole-program pass, report); in the LSP each
  request's and notification's method, file and position, never its text.
- **Focus:** `crash_context::focus(path)` sets the thread's current file:
  per-file units from their path, whole-program passes and load steps from
  the declaration they're on, the LSP dispatch from the request's document,
  the debounce worker from the file it rechecks. A unit's focus is also
  sent to the supervisor (`FOCUS`, keyed by thread name; the pool's threads
  are named `phpstan-rs-worker-<n>` through `ThreadPoolBuilder::thread_name`),
  and `isolated` clears it when the unit ends (`FOCUS_CLEAR`), so an idle
  thread never names a finished file. A declaration's focus (one per class,
  tens of thousands in a large project) stays in the thread-local. Setting
  either costs a thread-local write; a unit's also costs one socket
  message. Step 6 measures `check-project` over the Piwigo corpus with and
  without a DSN; the overhead must stay under 2 % of wall time, or the
  forwarding is batched per worker.

### 6.9 Environment hygiene

A developer with `PHPSTAN_RS_SENTRY_DSN` in their shell must not have gate,
corpus, performance or test runs supervised or reported: the performance
gate would time the supervisor, and a test's injected panic would reach the
real server. Every place that starts the binary removes
`PHPSTAN_RS_SENTRY_DSN`, `PHPSTAN_RS_SENTRY_CA_FILE` and every
`_PHPSTAN_RS_*` variable from the child: `xtask`'s `binary()` and
`runner()` commands, and one helper, `tests/common::phpstan_rs()`, which
`tests/cli.rs`, `lsp_exit_code.rs`, `version.rs` and the LSP harness use in
place of `Command::new(env!("CARGO_BIN_EXE_phpstan-rs"))`. The
crash-reporting tests set the variables back, pointing at their test
server. `tools/gate-runner` never calls `init`.

### 6.10 VS Code

- A setting `phpstan-rs.crashReporting.dsn` (string, empty by default,
  `"scope": "machine"`: only user settings on that machine set it, no
  workspace's `.vscode/settings.json` can, and Settings Sync doesn't carry
  it), whose description says what reports hold.
- `serverOptions.options.env` passes it as `PHPSTAN_RS_SENTRY_DSN` over
  `process.env`, and `onDidChangeConfiguration` (`extension.ts:133`)
  restarts the client when it changes, as for `serverPath`.
- In a remote window (SSH, a container, WSL) the server and its
  `machine`-scoped settings are the remote machine's, so reporting is
  turned on where the analysis runs.

### 6.11 Release workflow

- **`[profile.dist]`** inherits `release` with `debug = "limited"`
  (functions and line tables, all symbolication uses). `release` and `gate`
  are unchanged.
- **Build IDs:** `.cargo/config.toml` adds `-C link-arg=-Wl,--build-id` to
  `x86_64-unknown-linux-gnu`'s rustflags and gives
  `aarch64-unknown-linux-gnu` the same.
- **Build jobs** build with `--profile dist`, `PHPSTAN_RS_DIST=1` and
  `PHPSTAN_RS_BUILD_COMMIT=${{ github.sha }}`; packaging and the smoke
  check read `target/<target>/dist/`. They split the debug info:
  - Linux: `objcopy --only-keep-debug` into `phpstan-rs.debug`, then
    `objcopy --strip-debug --add-gnu-debuglink`
    (`aarch64-linux-gnu-objcopy` on the cross leg, from the binutils it
    installs);
  - macOS: `CARGO_PROFILE_DIST_SPLIT_DEBUGINFO=packed` writes a `.dSYM`,
    and `strip -S` strips the binary;
  - Windows: MSVC writes the `.pdb` beside the executable.

  Each job uploads the stripped binary and its debug file as an artifact
  (`actions/upload-artifact`, pinned by commit SHA like the workflow's
  other actions) and packages the stripped binary as today.
- **An `upload-debug-files` job** on ubuntu-24.04, after every build job,
  checks out with `fetch-depth: 0`, downloads the artifacts
  (`actions/download-artifact`, pinned) and `sentry-cli-Linux-x86_64`
  3.8.0, checks its SHA-256 (§4), then runs `debug-files check` on every
  binary (failing on one without a debug ID), `debug-files upload
  --include-sources --wait` on every binary and debug file (the binaries
  carry the unwind tables), and on a tag `releases new`, `set-commits
  --local` and `finalize` for `phpstan-rs@<version>+<sha>`. Secrets:
  `SENTRY_URL`, `SENTRY_ORG`, `SENTRY_PROJECT`, and `SENTRY_AUTH_TOKEN`, an
  organization token. A `workflow_dispatch` run uploads debug files and
  creates no release.

### 6.12 Server setup

Recorded in `docs/crash-reporting-server.md`, done once on the instance:

- a project `phpstan-rs`, platform Rust, with its own client key, rate
  limited to 60 events a minute;
- `sentry:store_crash_reports` ("Store Minidumps As Attachments") at 10 per
  issue, since the default 0 discards every dump after symbolication;
- the data scrubber and default scrubbers left on (they match key names
  and value patterns in the event; none of ours match, and they don't read
  plain attachments), and "Prevent storing of IP addresses" on;
- `sentry:attachments_role` raised from `member` to `manager`, so only
  maintainers download bundles and dumps;
- grouping as default plus the client's `["panic", location]` fingerprint;
  alerts on new and regressed issues;
- retention (`SENTRY_EVENT_RETENTION_DAYS`, 90 by default) also bounds the
  bundles: each machine sends one per crash location per 30 days, so plan
  for machines × distinct crashes × up to 10 MiB. An issue's user count is
  its installations; CI shows in the `ci` tags;
- an organization auth token for the release workflow, and a personal auth
  token for each maintainer who runs `crash-repro fetch`.

### 6.13 Developer tools

`tools/crash-repro`, a workspace member using the library, so the user's
binary doesn't carry it:

- **`replay <bundle>`** runs the bundle with its config and command and
  reports whether the same panic location fires, or, for a native crash,
  whether a child running it dies of the same signal.
- **`reduce <bundle>`** deletes AST nodes by span (statements, class
  members, function bodies, arguments, operands) across every file,
  keeping a deletion only while the crash still happens, then drops files
  nothing references. Panics are checked in-process under `catch_unwind`;
  native crashes in a child that writes its candidate to a state file
  before each attempt, so when it dies of the same signal the reducer
  records the candidate and starts another from the saved state. The
  result is a new bundle and a fixture directory ready to commit.
- **`fetch <issue>`** downloads an issue's latest bundle with `SENTRY_URL`,
  `SENTRY_ORG`, `SENTRY_PROJECT` and `SENTRY_FETCH_TOKEN`, a personal token
  with `event:read` and `project:read` whose user holds the attachments
  role: `GET /api/0/organizations/<org>/issues/<id>/events/latest/`, then
  `GET /api/0/projects/<org>/<project>/events/<event>/attachments/`, then
  the `repro.tar.gz` entry's `.../attachments/<attachment id>/?download=1`,
  following redirects (the token isn't sent across hosts) with
  `Accept-Encoding: identity`.
- **Bundles are untrusted input**, since anyone with the DSN can send one:
  extraction rejects absolute paths, `..`, links and device entries and
  stops past 256 MiB, into a fresh temporary directory; replay runs nothing
  but phpstan-rs on it.
- **`cargo xtask crash-debug-files [debug|release]`** uploads a local
  binary's debug files with `sentry-cli`, for a maintainer running
  development builds with a DSN (their panics already carry resolved
  frames; their minidumps need the files).
- **`docs/crash-triage.md`:** from an issue, `fetch`, `replay` on the
  current commit, `reduce`, commit the reduced fixture with the fix, and
  resolve the issue in the release carrying it, so a recurrence reopens as
  a regression; what `reproduces`, `bundle`, `unit` and `ci` mean, and when
  a bundle alone isn't enough (`sql: live`, `changed_since_crash`).

### 6.14 README

A "Crash reporting" section, in these words:

> phpstan-rs can send reports of its own crashes to a Sentry server you
> run. It is off by default. Set `PHPSTAN_RS_SENTRY_DSN` to the DSN of a
> Sentry project to turn it on (in VS Code, the
> `phpstan-rs.crashReporting.dsn` user setting; for another editor, the
> environment it starts `phpstan-rs lsp` with). `SENTRY_DSN` is ignored.
>
> A report contains the error message and stack trace, the command line,
> the effective configuration, and a copy of the PHP source files needed to
> reproduce the crash: the file being analysed and the files it depends
> on, up to 10 MiB compressed, including unsaved editor buffers, and the
> schema ext-sql reflected. It contains your file paths, OS and CPU
> architecture, a random id for your installation (kept in the cache
> directory; delete it to get a new one), and whether it ran in CI. It
> leaves out your hostname, your environment variables and the database
> connection settings (host, user, password, database name). A report of a
> hard crash (a stack overflow, an abort) also carries a snapshot of the
> program's thread stacks, which can hold fragments of whatever it was
> processing. Only turn it on for code you may send to that server.
>
> Reports that can't be sent are kept in the user cache directory
> (`phpstan-rs/crash-outbox` under `$XDG_CACHE_HOME`, `~/Library/Caches`
> or `%LOCALAPPDATA%`) and retried by later runs for 30 days; with
> reporting off nothing reads that directory, so delete it to discard
> what's left. With a private CA, point `PHPSTAN_RS_SENTRY_CA_FILE` at its
> certificate.

## 7. Tests

**Unit:**

- the hook's four paths; `isolated` recording location, backtrace and
  focus; a panic in a `par_iter` under an isolated unit recorded once
  through `carry`; `entry` printing and reporting a carried panic;
- `report.json` from each kind of crash, and the finisher's event from it
  (the `sentry` `test` feature capturing events in memory);
- the bundle walk's order (ancestors first, both directions for passes),
  the cap and omitted files, the config rewrite and its schema test,
  overlays, `SchemaCatalog::to_ddl` round-tripping through `parse_ddl`;
- `sent.json` under two concurrent writers and written only after a 2xx;
  the transport's status handling, DSN matching and `not_before`; two
  senders draining one outbox sending each entry once; the unwritable
  `ready/` fallback; the finisher's attempt counting;
- `xtask`'s command builders stripping the reporting variables.

**End to end** (`crates/phpstan-rs/tests/crash_reporting.rs`,
`required-features = ["crash-injection"]`). A test Sentry server on
`127.0.0.1` decodes envelopes over TLS, with a certificate from a test CA
that `PHPSTAN_RS_SENTRY_CA_FILE` points at, so every test also exercises
rustls and the CA option. The `crash-injection` feature (off by default)
makes the binary panic, panic outside isolation, overflow its stack, abort,
or allocate without bound on a named file or in a named pass; the unit
tests' existing hook (`tests::panic_injection`) becomes it, compiled under
`#[cfg(any(test, feature = "crash-injection"))]`.

- **Panics:** a check panic sends one event with its tags and a bundle of
  the right files and config, tagged `reproduces = yes`; a pass panic
  keeps the other passes' diagnostics and walks from the focus; a bundle
  whose first walk misses a declaration is widened until it reproduces;
  one depending on live SQL is `unchecked`; a panic's finisher never loads
  the project (its peak memory stays near the bundle's size).
- **Output:** an injected check panic appears in the JSON's
  `internal_errors`, checkstyle's `phpstan-rs.internal` error and a failing
  JUnit case, and the run exits 2; findings alone exit 1, a clean run 0.
  `check-project` into a reader that closes after one line exits 141 with
  nothing on stderr and, with a DSN, sends nothing.
- **Native crashes:** a stack overflow's fatal event arrives with the
  minidump and a bundle before the supervisor exits, with the exit status
  128 + the signal. With a canary environment variable and a canary
  `--set` value, the uploaded dump contains neither, on Linux, macOS and
  Windows. On Windows, an abort produces the "worker terminated" event.
- **OOM:** under a delegated cgroup memory limit (`systemd-run --user
  --scope -p MemoryMax=...`; skipped with a printed reason where the runner
  has none), an unbounded allocation ends in an `oom` report naming the
  file in progress; a plain `kill -9` of the worker produces none.
- **Supervisor:** exit codes pass through; `SIGTERM` is forwarded; the
  worker ends when the supervisor is killed (Linux, macOS, Windows); a
  reader of the worker's stdout sees the end as soon as the worker exits,
  while the supervisor is still finishing a report; a supervisor whose
  worker can't start runs the command itself; one whose server thread
  panics still relays the exit status and reports a later crash through
  the no-dump path; the socket is a path in a `0700` directory; a
  finisher's niceness is 19 on Unix and its priority class background on
  Windows; as a PID namespace's init (`unshare --user --map-root-user --pid
  --fork --mount-proc`, skipped where user namespaces are disabled) it
  stops on `SIGTERM` and leaves no zombies.
- **Upgrades:** with the running binary deleted (a copy of the test binary,
  removed after start), the supervisor still starts a finisher on Linux; a
  report left by one release and finished by another carries the first's
  release and `replayed_with` the second's.
- **Durability:** a report made while the server is down reaches it on the
  next run; a run killed right after an isolated panic leaves a report the
  next run sends; with the cache directory unwritable the temporary
  fallback is used, and with that unwritable too the run says once that
  reporting is off and runs as without a DSN.
- **Privacy:** a config with `ext.sql.reflection.password` and a `--set` of
  it produce a bundle, a crash state and a recorded command line holding
  neither value; the installation id is stable across runs and new after
  its file is deleted; the `ci` tags appear only with `CI` set.
- **Isolation:** `SENTRY_DSN` set and ours unset sends nothing and starts
  no extra process; `tests/common::phpstan_rs()` run with the DSN in the
  test's environment starts a child without it; `crash-sys` is the only
  crate whose source contains `unsafe`.
- **SQL cache:** a panic while the cache lock is held leaves later files'
  SQL reflection working.
- **LSP** (the stdio harness): 120 nested arrays in the startup scan and 60
  on `didChange` are checked without aborting; a panic in hover answers
  `InternalError` and the server keeps serving; one in a `didChange`'s
  check phase becomes the file's internal error and the next diagnostics
  match a fresh server's; one in its registry phase ends the worker with a
  report and a restart; a crashing `didChange` sends one event with the
  client tags and the overlay text, a repeat none, and an overlay edited
  right after the panic doesn't reach its bundle.
- **`crash-repro`:** a bundle from these tests replays and reduces to one
  that still reproduces; hostile bundles (`..`, absolute paths, links, a
  decompression bomb) are rejected.

`rust-ci.yml` and `cargo xtask gates`' quick tier run them with
`cargo test -p phpstan-rs --features crash-injection --test
crash_reporting`; `cargo hack --each-feature` covers the feature's build. A
`crash-reporting` job runs the same test on `macos-26` and
`windows-2025-vs2026`, the runners `release.yml` already uses, since the
supervisor, minidump and handle code differs per OS. Test-only
dependencies: `rcgen` for the CA, `rustls`'s blocking `ServerConnection`
over a `std::net::TcpListener` (no async runtime), `tar` and `flate2` to
read bundles.

## 8. Steps

1. **Panic isolation** (§6.1), with no Sentry involved, in the new
   `crash-context` crate: the hook, `isolated`, `carry` and its clippy
   entries, `entry`, internal errors, the isolated passes, member-index
   re-check and load steps, and the text, friendly and `check` output (a
   behaviour commit with its `tests/cli.rs` changes). Then, each its own
   behaviour commit with its test:
   - **1b** output through a writer, broken pipes exiting 141 (§6.1.5);
   - **1c** the SQL cache's poison recovery (§6.1.5);
   - **1d** internal errors in JSON, checkstyle and JUnit (§6.1.4);
   - **1e** the exit code, with its approved `tests/cli.rs` delta
     (§6.1.4);
   - **1f** the LSP's handlers on the pool, parse and check phases isolated
     (§6.1.6), with the LSP tests covering a handler panic.

   With each, its §9 edit.
2. **Release build:** `[profile.dist]`, the build-ID flags, the workflow's
   split, strip, artifacts and `upload-debug-files` job (§6.11), the
   secrets, and `docs/crash-reporting-server.md` (§6.12), checked against
   the instance.
3. **Supervisor:** the feature and dependencies, `init` and the roles, the
   cache directory and crash state (§6.3), the process pair for every
   command, the socket and messages, `crash-sys` with
   `release_std_handles`, releasing the handles, the running binary, signal
   forwarding and the worker's parent watch, PID 1, degraded starts
   (§6.2); environment hygiene (§6.9); the `SENTRY_DSN` test; and the cost
   with no DSN: `hyperfine` on `phpstan-rs --version` and on
   `check-project` over a small fixture directory, before and after,
   agreeing within noise, with the release binary's size before and after
   in the commit message.
4. **Reports and sending:** pending reports from panics (§6.4), the
   finisher without bundles, `enter_background`, the client, transport,
   outbox and draining (§6.6, §6.7), tags, context, breadcrumbs and the
   installation id (§6.8), `crash_report::exit`, the supervisor's lines.
5. **Bundles:** the plan, the builder, the schema's key registries and
   their test, the config rewrite, `SchemaCatalog::to_ddl`, `sent.json`,
   `__crash-replay` and widening (§6.5).
6. **Native crashes:** `crash_sys::attach`, named workers and `FOCUS`, the
   dump written into its report and scrubbed, the no-dump and OOM paths
   (§6.2.4, §6.4), the `crash-injection` feature and the end-to-end tests;
   the Piwigo overhead measurement (§6.8).
7. **LSP:** crash-state updates and overlays, focus and breadcrumbs from
   the dispatch loop, client tags, per-report finishers, and its tests.
8. **VS Code:** the setting (§6.10).
9. **`tools/crash-repro`** and `cargo xtask crash-debug-files`, with
   `docs/crash-triage.md` (§6.13).
10. **README** (§6.14).

## 9. Edits to `CRATE_SPLIT_PLAN.md`

Each goes in with the step that causes it:

- **B8, "No global panic hook":** landed early (step 1).
- **F1, "Panic isolation, on the pool":** landed early (steps 1 and 1f):
  the single hook, the internal error with file, message, location and an
  unresolved `backtrace::Backtrace`, and the LSP handlers on the pool with
  their parse and check phases isolated. F1 keeps isolating an update's
  registry phase, which needs C4's snapshots.
- **F1, "Internal errors are reported in every format":** landed early
  (step 1d), with §6.1.4's shapes.
- **F1, "The exit code":** landed early (step 1e).
- **F1, `AnalysisResult`:** its internal-error list is
  `Vec<InternalError>`, and each whole-program pass and load step is an
  isolated unit (step 1).
- **F2, result cache location:** `crash_report::cache_dir()` (step 3).
- **F3, ErrorFormatter set:** the existing formats already write through a
  `Write`, broken pipes ending the run quietly (step 1b); F3's formatters
  keep that contract.
- **F4, Binary:** the hidden `__crash-finish` and `__crash-replay`
  subcommands, and with a DSN every command running as a supervisor and a
  worker (step 3).
- **§3 crate list:** `crash-context` below `reflection` and `project`,
  depending on `backtrace` only (step 1); `crash-sys`, used by the app
  crate, the one crate allowed `unsafe` (step 3).

## 10. Done when

- With the DSN unset, or only `SENTRY_DSN` set, no process, thread or
  connection beyond today's exists, and the test server receives nothing.
- A panic in parse, facts, check, a whole-program pass or a load step, in
  the CLI or the LSP, reaches the test server as one event per location
  with a bundle `crash-repro replay` reproduces, and the rest of the run's
  results are still reported.
- An internal error shows in every output format and the run exits 2.
- A stack overflow on a worker thread reaches it as a fatal event with a
  minidump and a bundle, before the supervisor exits; the LSP's handlers
  no longer overflow at the depths that abort it today.
- A worker killed by the OOM killer under cgroup v2 reaches it as an `oom`
  report naming the files in progress; a `kill -9` doesn't.
- A report made while the server is down, or by a run killed right after,
  reaches it on the next run.
- With a DSN, every command runs as a supervisor and a worker, the only
  `unsafe` in the workspace is `crash-sys`'s three functions, and nothing a
  run starts outlives its supervisor.
- A release built by the workflow produces events whose frames Sentry shows
  with function, file and line, and source context from the uploaded
  sources.
