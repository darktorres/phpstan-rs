# Crash reporting

phpstan-rs reports its own crashes (panics, stack overflows, aborts) to a
self-hosted Sentry, with the source files needed to reproduce them. It is off
unless the user sets a DSN. This plan sits beside `CRATE_SPLIT_PLAN.md` and
builds on Stage F's panic isolation (§3.2).

Status: design, still being iterated. Nothing here is implemented.

## 1. Decisions

- **SDK: the `sentry` crate** (0.49.3 at the time of writing), from
  getsentry/sentry-rust. A self-hosted server works the same as sentry.io;
  only the DSN differs.
- **Transport: `reqwest` over `rustls`.** Features are additive, so dropping
  the default `native-tls` (OpenSSL on Linux) takes `default-features = false`.
  `rustls` is already in `Cargo.lock` through the MySQL client, the release
  binaries stay free of a `libssl.so` runtime dependency, and the
  `aarch64-unknown-linux-gnu` cross-build (`release.yml`) needs no arm64
  OpenSSL. The `reqwest` transport runs its own tokio runtime on a background
  thread; the rest of the program stays synchronous.
- **Crash events only.** No Sentry logs, no release health (sessions), no
  metrics, no performance tracing. Features:
  `backtrace`, `contexts`, `debug-images`, `panic`, `reqwest`, `rustls`.
- **Opt-in by `SENTRY_DSN`.** `sentry::init` reads `SENTRY_DSN`,
  `SENTRY_ENVIRONMENT` and `SENTRY_RELEASE`. With no DSN the client is
  disabled: no network, no crash-handler process, no per-file cost beyond a
  check that the hub is inactive. No DSN is compiled into any build.
- **Source is sent as-is.** The user opted in, and a report that reproduces
  beats one that protects the code; nothing is anonymised, hashed or scrubbed.
  The documentation of `SENTRY_DSN` says that reports carry project source
  files, file paths and configuration.
- **`server_name` is not sent.** The hostname doesn't help reproduce a crash.
- **Debug info is kept out of the binaries** and uploaded to Sentry from the
  release workflow (§3.7).
- **Minidumps are collected** for crashes a panic hook never sees (§3.6).

## 2. Today

- **The panic hook is swapped for a silent one** around every isolated batch:
  `main/check.rs:20-33` (`check`), `main/check_project.rs:131-134`
  (`check-project`), `project.rs:1644-1648` (the LSP's
  `check_paths_isolating_panics`) and `composer/src/vendor.rs:64-67` (vendor
  discovery). A panic inside any of those windows reaches no hook that could
  report it, and those windows are where nearly every caught panic happens.
- **Per-file `catch_unwind`:** `project/session.rs:578` (parse; the panic
  becomes a skipped file), `project/session.rs:707` (check), `project.rs:1561`
  (property-assign facts; the file contributes nothing), `project.rs:1673`
  (LSP check), `main/check.rs:54` (parse-only `check`).
- **`process::exit`:** `main.rs:83` (LSP exit without shutdown), `main.rs:91`
  (`convert-baseline` error), `main/check_project.rs:116,123` (config and
  prepare errors, before any analysis). An exit skips the client's `Drop`,
  which is what flushes queued events.
- **Aborts:** a file with 60 nested arrays aborts the LSP on `didChange` with a
  stack overflow on its 2 MiB debounce thread; 120 abort it during the startup
  scan (`CRATE_SPLIT_PLAN.md`, Stage F, "Panic isolation, on the pool"). A
  32,000-term `.` chain aborts `check-project` (Stage D).
- **`[profile.release]`** has no debug info (`lto = "thin"`,
  `codegen-units = 1`). A report would carry function names from the symbol
  table but no file and line.
- **Release targets:** `x86_64`/`aarch64-unknown-linux-gnu` (ubuntu-24.04, the
  aarch64 one cross-compiled), `aarch64`/`x86_64-apple-darwin`,
  `x86_64-pc-windows-msvc`. Binaries go to GitHub releases, and the VS Code
  extension downloads them (`editors/vscode/src/serverDownload.ts`). Nothing is
  on crates.io (`publish = false`).

## 3. Design

### 3.1 Client

`main` initialises the client first, with `release` set from the package
version and `send_default_pii` off, and holds the guard for the whole run. The
two `process::exit` paths that can follow an analysis (`main.rs:83`, and any
exit Stage F's exit codes add after a run) flush the client first; the
pre-analysis exits in `check_project.rs` have nothing queued.

### 3.2 Panic hook

Stage F replaces the hook swapping with one hook installed once, quiet on a
thread that is inside an isolated check. Reporting goes into that hook: it
always hands the panic to `sentry::integrations::panic::panic_handler` (a
no-op without a client), and prints the default message only when the thread
isn't inside an isolated check. The stack trace is taken where the panic
happened, not where `catch_unwind` caught it.

If crash reporting lands before Stage F, each of the four silent hooks in §2
becomes `panic_handler` instead of `|_| {}`, and Stage F later folds them into
its single hook. `composer` then depends on `sentry` for vendor discovery's
hook; Stage F removes that hook ("No global panic hook"), and with it the
dependency.

### 3.3 Which file

Every per-file isolation site in §2 runs its file inside `sentry::with_scope`,
setting the file's path relative to the project root. The scope is pushed on
the worker thread that checks the file, so the hook's event carries it. The
same site sets the crash marker (§3.6).

### 3.4 Event contents

- **Tags:** subcommand (`check`, `check-project`, `lsp`), rule level, the
  optional rules enabled, `pcre-validate` on or off, analysed and context file
  counts, the crashing file's path.
- **Breadcrumbs:** the run's phase (config, load, property-assign facts,
  check, whole-program passes); in the LSP, the last requests and
  notifications (method, file, position), bounded to the SDK's default count.
- **Attachments:**
  - the crashing file, as read from disk or from the LSP's overlay;
  - for a check panic, the files declaring what it references: each name in
    the file's `FileState::references`, resolved through the registry's
    `Provenance::file`, plus those declarations' parents, interfaces and
    traits, transitively, stopping at a total size cap (to decide, §5);
  - the configuration as loaded: the effective settings after includes,
    `--set` and `--level`, not only the raw `phpstan.neon`.

### 3.5 Internal errors

Stage F gives a caught panic an internal error carrying the file, the panic
message and location, and the captured backtrace. That record and the Sentry
event come from the same hook call, so the two describe the same panic; the
event doesn't replace the internal error in any output.

### 3.6 Minidumps and the crash marker

A stack overflow or abort runs no panic hook. `sentry-rust-minidump` starts a
crash-handler process beside the main one (only when a DSN is set); on a
crash it writes a minidump and sends it as an event attachment. The main
process records the file it is checking (§3.3) where that event can read it,
so the report names the file and attaches it and its dependencies as in §3.4.
Where the marker lives follows Stage F2's default cache location.

### 3.7 Debug files

Release builds keep full debug info in separate files, and the binaries are
stripped:

- Linux: debug info split out of the binary, matched by GNU build ID;
- macOS: `.dSYM` bundles;
- Windows: `.pdb`, which MSVC already produces separately.

`release.yml` uploads them with `sentry-cli debug-files upload` for every
target, with the Sentry URL, auth token, org and project as repository
secrets. Minidump stack walking needs the same files. The uploaded files must
come from the exact build whose stripped binary is published.

## 4. Follow-up: reduction

Once reports carry their files, a reducer attaches a minimal reproduction next
to the originals. The panic's location in phpstan-rs's own source identifies
the crash. The reducer deletes AST nodes by span (statements, members,
function bodies, arguments, operands) across the crashing file and its
attached dependencies, re-checking after each deletion and keeping it only if
the same location still panics. It runs under a time budget, in the
background in the LSP; if it doesn't finish, the originals already
reproduce. After an abort it can't run in the dead process, so the next
launch reduces the marked file in child processes, where an attempt that
overflows the stack only kills that child.

## 5. Open questions

- The total size cap for attached source files.
- Whether `crash-handler` installs an alternate signal stack on every thread
  or only the one that sets it up; a stack overflow on a rayon worker or the
  LSP's debounce thread has to be caught.
- Whether the Linux release binaries carry a GNU build ID without an explicit
  `-Wl,--build-id`.
- Why adding `sentry` pulled `actix-web` into `Cargo.lock`, and whether the
  §1 feature list drops it (`cargo tree -i actix-web`).
- Whether reporting lands before or after Stage F (§3.2).
- Whether the LSP reports the client's name and version (from `initialize`)
  as a tag.
