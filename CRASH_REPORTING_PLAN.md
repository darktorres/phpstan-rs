# Crash reporting

phpstan-rs reports its own crashes (panics, stack overflows, aborts) to a
self-hosted Sentry, with a bundle of the inputs that reproduce each one. It is
off unless the user sets a DSN. This plan sits beside `CRATE_SPLIT_PLAN.md`:
it pulls Stage F's single panic hook and internal-error record forward in
their final form (§5), the way `GATE_SPEED_PLAN.md` pulls items forward.

Status: design. Nothing here is implemented.

## 1. Decisions

- **SDK: `sentry` 0.49.3** (getsentry/sentry-rust), and **`sentry-minidump`
  0.49.3** from the same repository for native crashes (the older
  `sentry-rust-minidump` is deprecated in its favour). A self-hosted server
  works the same as sentry.io; only the DSN differs.
- **Transport: `reqwest` over `rustls`.** Features are additive, so dropping
  the default `native-tls` (OpenSSL on Linux) takes `default-features = false`.
  `rustls` 0.23 and its `aws-lc-rs` provider are already in `Cargo.lock`
  through the MySQL client, so CI already builds them for every target. The
  release binaries stay free of a `libssl.so` runtime dependency, and the
  `aarch64-unknown-linux-gnu` cross-build needs no arm64 OpenSSL. Certificates
  are checked by `rustls-platform-verifier`, which reads the OS trust store, so
  a self-hosted server behind a private CA works once that CA is installed
  system-wide. The `reqwest` transport runs its own tokio runtime on a
  background thread; the rest of the program stays synchronous.
- **Crash events only.** No Sentry logs, no release health (sessions), no
  metrics, no performance tracing. Features: `backtrace`, `contexts`,
  `debug-images`, `panic`, `reqwest`, `rustls`. The probe build of exactly this
  set pulls in no `actix-*` crate; the `actix-web` seen in `Cargo.lock` earlier
  came from the default feature set.
- **Opt-in by `PHPSTAN_RS_SENTRY_DSN`, never `SENTRY_DSN`.** `sentry::init`
  falls back to `SENTRY_DSN`, `SENTRY_RELEASE` and `SENTRY_ENVIRONMENT` for any
  option left unset, and a PHP project's CI or container often sets
  `SENTRY_DSN` for the application itself; reading it would send phpstan-rs's
  crashes, with the project's source, to the application's Sentry. So
  phpstan-rs reads only its own variable, calls `sentry::init` only when it is
  set, and sets `dsn`, `release` and `environment` explicitly so no fallback
  applies. Unset means no client, no crash-reporter process, no network, and no
  per-file work beyond one inactive-hub check. No DSN is compiled into any
  build.
- **The VS Code extension** gets a `phpstan-rs.crashReporting.dsn` setting
  (empty by default), passed to the server process as
  `PHPSTAN_RS_SENTRY_DSN`; an editor started from the desktop doesn't see a
  shell's environment.
- **Source is sent as-is.** The user opted in, and a report that reproduces
  beats one that protects the code; nothing is anonymised, hashed or scrubbed.
  The documentation of the variable and of the VS Code setting says that
  reports carry project source files, file paths and configuration.
- **The hostname is not sent.** The contexts integration fills `server_name`
  with it; `before_send` clears it.
- **Reduction runs on the developer's side** (§4.9), from the bundle, not on
  the user's machine: the bundle already holds the full inputs, so reducing
  there would add nothing for privacy and would cost the user CPU time or a
  background process.
- **Debug info is kept out of the binaries** and uploaded to Sentry from the
  release workflow (§4.8).
- **Minidumps are collected** for crashes no panic hook sees (§4.7).

## 2. Today

- **The panic hook is swapped for a silent one** around every isolated batch:
  `main/check.rs:20-33` (`check`), `main/check_project.rs:131-134`
  (`check-project`), `project.rs:1644-1648` (the LSP's
  `check_paths_isolating_panics`) and `composer/src/vendor.rs:64-67` (vendor
  discovery). A panic inside those windows reaches no hook that could report
  it, and those windows are where nearly every caught panic happens.
- **Per-file `catch_unwind`:** `project/session.rs:578` (parse; the panic
  becomes a skipped file, message dropped), `project/session.rs:707` (check),
  `project.rs:1561` (property-assign facts; the file contributes nothing,
  message dropped), `project.rs:1673` (LSP check), `main/check.rs:54`
  (parse-only `check`). A caught panic keeps its message at most; no location
  and no backtrace.
- **Not isolated:** the LSP's `didOpen`, watched-file events, hover, inlay
  hints, code actions and completion probes run on the main or debounce thread.
  A panic there unwinds holding the `Server` mutex, and the next
  `lock().unwrap()` panics too (Stage F, "Panic isolation, on the pool").
- **`process::exit`:** `main.rs:83` (LSP `exit` without `shutdown`),
  `main.rs:91` (`convert-baseline` error), `main/check_project.rs:116,123`
  (config and prepare errors, before any analysis). An exit skips `Drop`, which
  is what flushes a Sentry client.
- **Aborts:** a file with 60 nested arrays overflows the LSP's 2 MiB debounce
  thread on `didChange`; 120 abort it during the startup scan. A 32,000-term
  `.` chain aborts `check-project` (Stage D).
- **Build IDs:** the x86_64 Linux build links with clang + mold
  (`.cargo/config.toml`), and mold writes no GNU build ID unless asked; the
  debug binary at `56c3beab` has none (`readelf -n`). The aarch64 cross-build
  links with Ubuntu's `aarch64-linux-gnu-gcc`, which passes `--build-id` by
  default.
- **`[profile.release]`** has no debug info (`lto = "thin"`,
  `codegen-units = 1`), and `[profile.gate]` inherits it.
- **Release targets:** `x86_64`/`aarch64-unknown-linux-gnu` (ubuntu-24.04, the
  aarch64 one cross-compiled), `aarch64`/`x86_64-apple-darwin`,
  `x86_64-pc-windows-msvc`, built with `cargo build --release`, gzipped, and
  attached to the GitHub release. The VS Code extension downloads them
  (`editors/vscode/src/serverDownload.ts`) and starts `phpstan-rs lsp` with no
  environment of its own (`extension.ts:67`). Nothing is on crates.io
  (`publish = false`).
- **Configuration** is a TOML `ConfigFile` (`crates/config`): the file's table
  with old keys moved and `--set` applied. The LSP loads it per workspace
  (`lsp/workspace_config.rs:68`).

## 3. What the crates do (read from their source)

- **`sentry-minidump`** spawns the current executable again as a crash-reporter
  process from inside `sentry::init`, with the app's arguments and an
  environment variable marking it. In the reporter, `sentry::init` never
  returns: it runs the minidump server and exits. Code before `sentry::init`
  runs in both processes. The app pings the reporter every 2.5 s, and the
  reporter exits after 5 s without one. The spawn uses `Command` defaults, so
  the reporter inherits stdin, stdout and stderr unless `on_process` changes
  them. Scope changes don't cross the process boundary on their own; the
  integration forwards breadcrumbs, tags, extras and the user over its socket.
  The minidump arrives in the reporter's `before_capture(scope, path)`, where
  our code can add attachments before the fatal event is sent.
- **`crash-handler`** (under `minidumper-child`) interposes `pthread_create` on
  Linux and macOS, so every thread gets its own alternate signal stack and a
  stack overflow on a rayon worker or the debounce thread is caught. The
  interposer is linked into the binary whether or not a DSN is set; it is a
  per-thread `mmap` and one `sigaltstack` call. The signal handlers are
  installed only when the integration attaches. On Windows it uses a vectored
  exception handler, which sees `EXCEPTION_STACK_OVERFLOW`; on macOS it hooks
  `SIGABRT` as well.
- **`sentry-backtrace`** turns a `backtrace::Backtrace` into a Sentry
  stacktrace (`backtrace_to_stacktrace`), with instruction addresses the server
  symbolicates against uploaded debug files.
- **`sentry-panic`** exposes `PanicIntegration::event_from_panic_info`, so a
  hook of our own builds the same event its hook would.
- **Defaults:** `shutdown_timeout` 2 s, `max_breadcrumbs` 100, a transport
  queue of 30 envelopes.

## 4. Design

### 4.1 Client

`main` starts with `crash_report::init()`, before `Cli::parse` and the dhat
profiler, because the crash-reporter process re-executes the binary and must
diverge before any app work. With `PHPSTAN_RS_SENTRY_DSN` unset it returns
`None` and nothing else in this plan runs.

With it set, the options are:

- `dsn` from the variable; `release` = `phpstan-rs@<version>+<commit>`, the
  commit taken from `PHPSTAN_RS_BUILD_COMMIT` at compile time
  (`option_env!`, set by the release workflow), and `+dev` when absent;
- `environment` = `release` for a build of the `dist` profile (§4.8,
  `PHPSTAN_RS_DIST` at compile time), `development` otherwise;
- `default_integrations = false`, then the backtrace, contexts, debug-images
  and minidump integrations added by hand: the panic integration's own hook is
  not installed, because the single hook (§4.2) calls it;
- `send_default_pii = false`; `before_send` clears `server_name`;
- `in_app_include = ["phpstan_rs", "syntax", "types", "phpdoc", "sql",
  "diagnostics", "composer", "config", "stub_sources"]`, so Sentry folds std,
  rayon and the SDK out of the trace;
- `shutdown_timeout` 30 s: a bundle can be megabytes, and a CLI run waits for
  its reports before exiting, printing
  `phpstan-rs: sending N crash report(s) to <host>` to stderr first so the
  wait is visible.

The guard lives in `main` for the whole run. The `process::exit` paths flush
first through one `crash_report::exit(code)` that `main.rs:83` and `:91` call;
the exits in `check_project.rs:116,123` stay as they are, having nothing
queued (they become `crash_report::exit` too, so no future exit after analysis
is missed).

### 4.2 Panic hook (pulled forward from Stage F)

One hook, installed once in `main` after `init`, replaces all four swaps in
§2. A thread-local marks a thread as being inside an isolated check
(`crash_report::isolated(file, || ...)` sets and clears it; the per-file
sites in §2 call it around their `catch_unwind`). The hook:

- **on an isolated thread** prints nothing and stores the panic's message,
  location and an unresolved `backtrace::Backtrace` in a thread-local, which
  the catch site takes into the file's internal error (§4.3). The backtrace is
  captured when a client is active or `RUST_BACKTRACE` is set, and only
  resolved when printed or sent.
- **anywhere else** prints the default message (the previous hook, kept) and,
  with a client, captures the event through
  `PanicIntegration::event_from_panic_info` with the current breadcrumbs, then
  flushes with a 5 s limit, since a panic outside isolation usually takes the
  process down.

Vendor discovery's swap is deleted rather than converted: its parse already
returns errors (B8's "No global panic hook"). With it goes the only reason
`composer` would need `sentry`.

### 4.3 Internal errors (pulled forward from Stage F)

Every per-file `catch_unwind` in §2 turns a panic into an internal error
`{ file, phase, message, location, backtrace }`, `phase` being parse,
property-assign facts, check or whole-program pass. The parse site stops
dropping the panic into a skipped file, and the facts site stops dropping it
into an empty contribution; each records the error. How internal errors are
shown in the CLI formats, the exit code and the LSP stays Stage F's; this
plan needs only the record.

### 4.4 From internal errors to events

- **CLI (`check`, `check-project`):** after analysis and before the report,
  internal errors are grouped by panic location. Each group becomes one event:
  message and stack trace from the group's first error, tags (§4.5), the list
  of every file in the group (paths only, as an extra), and one bundle (§4.6)
  built for the group's smallest file, the cheapest to reproduce.
- **LSP:** a `phpstan-rs-crash-report` thread takes internal errors over a
  channel. Whoever holds the `Server` lock computes the bundle's file list and
  copies any overlay text it includes (cheap lookups), then hands off;
  reading, archiving and sending happen off the lock. Within a session, a
  panic location already reported is only counted, not sent again.
- **Across runs:** `<cache>/phpstan-rs/crash-reports/sent.json` records
  `(release, panic location, crashing file's content hash)` for each bundle
  sent. A repeat within 30 days still sends the event, so Sentry's counts stay
  true, but without the bundle. `<cache>` is the user cache directory Stage F2
  names for the result cache (`$XDG_CACHE_HOME` or the platform equivalent),
  independent of `tmp_dir`, which the reporter process can't know before the
  config loads.

### 4.5 Tags, context and breadcrumbs

- **Tags:** `command` (`check`, `check-project`, `lsp`), `level`,
  `phase`, `pcre_validate`, `lsp.client` and `lsp.client_version` (from
  `initialize`'s `clientInfo`), `bundle` (`attached`, `already_sent`,
  `too_large`, `failed`).
- **Context `run`:** analysed and context file counts, the enabled optional
  rules, the PHP version range, whether ext-sql reflects a live database.
- **Breadcrumbs:** each run phase (config, discovery, load, property-assign
  facts, check, whole-program passes, report); in the LSP, each request and
  notification's method, file and position, never its text. The SDK keeps the
  last 100. The minidump integration forwards them to the reporter, so a
  native crash carries them too.

### 4.6 Reproduction bundle

One `repro.tar.gz` attachment per event, built by one function for panics
(from the live project) and aborts (§4.7):

```
manifest.json
phpstan-rs.toml          the effective config, rewritten (below)
project/<path>           the files, at their paths under the project root
external/<n>/<name>      files outside the root, mapped in the manifest
```

- **`manifest.json`:** the phpstan-rs release and commit, the command line,
  the crash (kind, phase, message, panic location or signal), and each file
  with its role (`crashing`, `dependency`, `stub`, `composer`), size, SHA-256
  and whether it came from an unsaved editor buffer; files left out for the
  size cap, with their roles and sizes.
- **Files:** the crashing file, from the LSP overlay when there is one; then
  the files declaring what it references, found through each name in its
  `FileState::references` and the registry's `Provenance::file`, breadth-first
  by reference distance, with a class's parents, interfaces and traits always
  one level ahead of anything else at the same distance; the user's stub files
  (`stub_dirs`); `composer.json` and `composer.lock` from each discovered root
  (they set the PHP version range). The breadth-first walk fills the bundle up
  to the cap rather than stopping at a fixed depth, which maximises the
  chance the bundle reproduces.
- **Cap:** 10 MiB compressed, 64 MiB uncompressed, whichever comes first. Self-
  hosted Relay accepts 100 MiB attachments by default, but the CLI waits for
  the upload (§4.1); 10 MiB of gzipped PHP is roughly 60 MiB of source, far
  beyond a typical closure. A crashing file larger than the cap alone is sent
  as the manifest only, tagged `too_large`.
- **Config:** the effective `ConfigFile` table (old keys moved, `--set`
  applied) serialised back to TOML, with path-valued keys rewritten: the
  analysed paths become the crashing file, the dependency files become
  `scan_files`, stub directories point into the bundle, and keys naming
  files the crash doesn't read (the baseline, output files) are dropped.
  `config::Schema` gains a list of its path-valued keys so the rewrite doesn't
  hard-code them.
- **Archive:** `tar` + `flate2` (`flate2` is already in the lock).

### 4.7 Minidumps and the crash reporter

- **Integration:** `MinidumpIntegration` with `server_env_var`
  `_PHPSTAN_RS_CRASH_REPORTER`, `process_name` `phpstan-rs crash reporter`,
  `crashes_dir` `<cache>/phpstan-rs/crashes`, and `on_process` setting the
  reporter's stdin and stdout to null, since under `lsp` they are the protocol
  pipes; stderr stays inherited.
- **What the app tells the reporter:** at start, a crash-state directory
  `<cache>/phpstan-rs/crash-state/<pid>-<uuid>/` holding `state.json` (the
  command line, working directory, project root, config path, LSP client), set
  as an extra. Before each file's parse or check, an extra
  `in_progress.<thread name>` with that file's path; the worker pool's threads
  are named (`phpstan-rs-worker-<n>`) for this. In the LSP, the debounce worker
  writes each overlay it rechecks into the state directory's `overlays/`, and
  `didClose` and `didSave` remove it, so the reporter sees unsaved text. The
  app deletes its state directory on a normal exit; a new process removes
  directories whose pid is gone.
- **In the reporter's `before_capture`:** it runs
  `phpstan-rs __crash-bundle <state dir> <in-progress paths>`, a hidden
  subcommand, as a child with a 120 s limit. The child loads the project from
  `state.json` and the overlays (parse and index only, no checking), and builds
  the bundle with §4.6's function, every in-progress file marked `crashing`
  since the dump alone doesn't say which thread's file caused it. It runs as a
  child so a crash during that load can't take the report with it. If the
  child fails or times out, the reporter attaches the in-progress files and
  the effective config directly, tagged `bundle=failed`.
- **Cost with a DSN set:** one extra process per run, a socket message per
  file, and in the LSP one file write per recheck of a dirty buffer.

### 4.8 Debug files and the release workflow

- **`[profile.dist]`** inherits `release` with `debug = "limited"` (functions
  and line tables; no types or variables, which symbolication doesn't use).
  `release` and `gate` are unchanged, so local and gate builds keep their
  build times.
- **Build IDs:** `.cargo/config.toml` adds `-C link-arg=-Wl,--build-id` to
  `x86_64-unknown-linux-gnu`'s rustflags and gives
  `aarch64-unknown-linux-gnu` the same flag, so both Linux binaries carry one
  explicitly.
- **`release.yml`** builds with `--profile dist`, sets `PHPSTAN_RS_DIST=1`
  and `PHPSTAN_RS_BUILD_COMMIT=${{ github.sha }}`, then per target:
  - Linux: `objcopy --only-keep-debug` into `phpstan-rs.debug`, then
    `objcopy --strip-debug --add-gnu-debuglink` on the binary
    (`aarch64-linux-gnu-objcopy` for the cross leg, from the binutils it
    already installs);
  - macOS: `CARGO_PROFILE_DIST_SPLIT_DEBUGINFO=packed` produces a `.dSYM`,
    and the binary is stripped with `strip -S`;
  - Windows: MSVC writes the `.pdb` beside the executable.

  A pinned `sentry-cli`, downloaded with a checked SHA-256 like the workflow's
  other pins, runs `debug-files upload --include-sources` on the stripped
  binary and its debug file (the binary carries the unwind tables the server
  also reads). On a tag it runs `releases new`, `releases set-commits --auto`
  and `releases finalize` for `phpstan-rs@<version>+<sha>`. Secrets:
  `SENTRY_URL`, `SENTRY_AUTH_TOKEN`, `SENTRY_ORG`, `SENTRY_PROJECT`. The
  packaged, gzipped binary is the stripped one that was uploaded. A
  `workflow_dispatch` run uploads debug files but creates no release.

### 4.9 Developer tools

A `tools/crash-repro` binary (a workspace member depending on the `phpstan-rs`
library, so the user's binary doesn't carry it):

- **`replay <bundle>`** extracts the bundle into a temporary directory and
  runs the check in-process with the bundle's config, reporting whether the
  same panic location fires (or, for an abort, whether the child running it
  dies the same way).
- **`reduce <bundle>`** shrinks the bundle while it still reproduces: it
  deletes AST nodes by span (statements, class members, function bodies,
  arguments, operands) across the crashing and dependency files, keeping each
  deletion only if the crash still happens, then drops files nothing
  references any more. Panic crashes are checked in-process under
  `catch_unwind`. Abort crashes are checked in a worker process that writes
  its candidate to a state file before each attempt; when the worker dies of
  the same signal, the supervisor records that candidate as reproducing and
  starts a new worker from the saved state. The result is a new bundle and a
  ready-to-commit fixture directory.

### 4.10 Tests

- **Unit:** the hook's isolated and non-isolated paths (recorded, not
  printed; printed and captured), with `sentry`'s `test` feature capturing
  events in memory; the bundle builder (closure order, ancestors first, the
  cap and `omitted`, path rewriting, overlays); grouping and the `sent.json`
  store.
- **End to end:** a test HTTP server on `127.0.0.1` stands in for Sentry and
  decodes the envelopes. `tests/cli.rs` runs `check-project` with
  `PHPSTAN_RS_SENTRY_DSN` pointing at it and a file that panics, and checks
  the event, its tags and the bundle's contents; the same with `SENTRY_DSN`
  set and ours unset checks that nothing is sent. The binary's crash
  injection (a `crash-injection` cargo feature, off by default: panic or
  overflow the stack on a named file) drives these and the minidump test,
  which expects a fatal event with a minidump and a bundle.
- **LSP:** the existing stdio harness with the test server: a `didChange`
  that crashes produces one event carrying the client tag and the overlay
  text, and a repeat produces none.
- **Replay:** a bundle from the end-to-end test replays and reduces to a
  bundle that still reproduces.

## 5. Order

One commit per step, each checked by fmt, clippy and its touched modules'
tests; the end-to-end tests run at step 7 and again at the end.

1. **Single panic hook and internal-error record** (§4.2, §4.3), pulled
   forward from Stage F with no Sentry involved: the four swaps go, vendor
   discovery's with them, and the parse and facts sites stop dropping
   panics. `CRATE_SPLIT_PLAN.md` records the two items as landed early, in
   the commit that lands them.
2. **Build:** `[profile.dist]`, the build-ID flags, and the release
   workflow's split, strip and upload (§4.8), with the secrets in place.
3. **Client:** dependency, `init`, options, `exit`, the non-isolated capture
   path, and the `SENTRY_DSN` isolation test.
4. **Events from internal errors:** grouping, tags and context, the CLI
   flush message (§4.4, §4.5), without bundles.
5. **Bundles:** the builder, the schema's path-key list, the effective config,
   `sent.json` (§4.6).
6. **LSP:** the report thread, breadcrumbs, the client tag, session dedupe.
7. **Minidumps:** the integration, crash-state directory, named workers,
   overlays on disk, `__crash-bundle`, stale cleanup (§4.7), and the
   end-to-end tests with `crash-injection`.
8. **VS Code:** the `phpstan-rs.crashReporting.dsn` setting.
9. **`tools/crash-repro`:** replay, then reduce.
10. **Documentation:** a README section on crash reporting: the variable, the
    VS Code setting, and exactly what a report contains.

## 6. Done when

- With the DSN unset (and with only `SENTRY_DSN` set), no process, thread or
  connection beyond today's exists, and the test server receives nothing.
- A panic in parse, facts, check or a whole-program pass, in the CLI or the
  LSP, reaches the test server as one event per location with a bundle that
  `crash-repro replay` reproduces.
- A stack overflow on a worker thread and on the LSP's debounce thread each
  reach it as a fatal event with a minidump and a bundle.
- A release built by the workflow produces events whose frames Sentry shows
  with function, file and line, and source context from the uploaded sources.
