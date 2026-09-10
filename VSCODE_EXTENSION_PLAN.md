# Path to a real-time VSCode extension

This is a phased execution plan, not an idea backlog like BEYOND_PHPSTAN.md
- every item here is grounded in phpstan-rs's actual current `src/lsp.rs`
(and friends) behavior, produced by six independent audit lenses (protocol/
transport correctness, LSP method coverage, real-time-perf MVP scoping,
diagnostics push/debounce, workspace/config discovery, and the VSCode
client scaffold itself), each finding re-verified against the real files
before being kept. 47 of 47 raw findings survived verification - the six
lenses converged heavily on the same handful of root problems from
different angles, so this document merges ~47 raw findings down to the
~20 genuinely distinct action items below, grouped into four phases.

The core diagnosis, in one sentence: **the LSP server today behaves like a
per-file hover/definition helper, not like `check-project` running live**
- it never loads config/composer context, never scans vendor/ the way the
CLI does, never proactively publishes diagnostics, and runs on a single
blocking thread with no debounce - and **the VSCode extension client
doesn't exist anywhere in this repo yet**, confirmed absent by every lens
that checked (`find` for `package.json`/`*vscode*`/`*.vsix`/`client`,
all outside `vendor/`, zero hits).

---

## Phase 1 — Make the server's own diagnostics trustworthy

Nothing downstream matters if editor diagnostics don't match what
`check-project` reports on the identical tree. Right now they don't, for
four independent reasons.

### 1.1 Wire config/composer/opt-in-check resolution into LSP mode

`Server::new`/`load_workspace` (`src/lsp.rs:121-167`) build a bare
`Project::new()` with every opt-in check at its default-off state - no
`phpstan-rs.toml` (`Config::load` has exactly one production call site,
`src/main/check_project.rs:257`, gated on `check-project`'s `--config`
flag), no composer/vendor context (`composer::discover_vendor_context`
is never called from `src/lsp.rs` at all, confirmed by grep), no
`php_version`, no baseline. `Command::Lsp` (`src/main.rs:184`) takes zero
arguments. Meanwhile `run_check_project_command` resolves all of this
(`src/main/check_project.rs:108-116,256-337`) before checking a single
file. Result: a team's CI-enforced settings (deprecated-usage checks,
internal-usage checks, a baseline of already-triaged findings) silently
don't apply in the editor, and vendor classes aren't resolved as type
context at all - both false positives and false negatives relative to CI.

**Fix**: give `Command::Lsp` the same config surface as `CheckProject`
(discover `phpstan-rs.toml` from the LSP-provided workspace root, call
the same `composer::discover_vendor_context`/`detect_phpstan_dependency`
+ `project.set_check_*`/`set_php_version` sequence `check_project.rs`
already runs) before `load_workspace`. **Effort: medium.**

### 1.2 Filter the workspace scan; split analyzed vs. context-only files

`collect_php_files` (`src/project.rs:549-560`), called unfiltered by
`load_workspace` (`lsp.rs:138-167`), recurses into every subdirectory
with zero exclusions - no `vendor/`, `.git/`, `node_modules/` skip - and
loads every file via the same full `load_file` path used for first-party
code. Contrast with `check-project`'s deliberate `analyze_files` (checked)
vs. `scan_dirs`/vendor-context (loaded for type resolution only, never
diagnosed) split (`check_project.rs:81-106,329-337`). For a real
Composer project (routinely 10k-100k+ files under `vendor/`), this means
severe startup latency *and* semantically wrong results - vendor code
gets treated as checkable first-party code.

**Fix**: give `load_workspace` the same checked/context-only split,
sourced from the resolved config's `paths`/`scan_paths` (once 1.1 lands)
or a hardcoded default exclusion (`vendor/`, `.git/`, `node_modules/`) at
minimum, and load `vendor/` only through the existing autoload-map-driven
`discover_vendor_context` (a small, precise reachable-symbol set, not a
blind recursive walk). **Effort: medium.**

### 1.3 Publish diagnostics for the whole workspace after loading, not just touched files

`load_workspace` only populates the registry - it never runs the checker.
`"initialized" => {} // nothing to do yet` (`lsp.rs:629`). Diagnostics are
only ever produced by `handle_open_or_change`, reachable solely from
`didOpen`/`didChange` (`lsp.rs:169-206,630-650`). Opening a project fresh
shows an empty Problems panel regardless of how many real errors exist,
until each affected file happens to get opened or edited in that session
- for the majority of files in a real project, that's never.

**Fix**: after `load_workspace` (and 1.1/1.2) complete, run the checker
over every analyzed file and publish `textDocument/publishDiagnostics`
for each one with findings, matching what `check-project` would report
on the same tree. **Effort: medium** (small once 1.1/1.2 exist).

### 1.4 Map diagnostic level to LSP severity instead of hardcoding ERROR

`to_lsp_diagnostic` (`src/lsp/protocol.rs:119-132`) hardcodes
`DiagnosticSeverity::ERROR` for every diagnostic and never reads
`Diagnostic::level`, the same field `check-project`'s `apply_level_filter`
(`check_project.rs:27-35`) already treats as meaningfully graded. Every
finding renders identically as a red squiggle regardless of severity.

**Fix**: map `Diagnostic::level` to a `DiagnosticSeverity` tier (lower/
more speculative levels → WARNING/HINT) in `to_lsp_diagnostic`.
**Effort: small.**

---

## Phase 2 — Concurrency and responsiveness

Even with Phase 1 done, the server is architecturally unable to feel
"real-time": one thread, one blocking read, full rebuild on every
keystroke, no way to answer a hover while a recheck is running.

### 2.1 Move off the single blocking read loop

`run_loop` (`lsp.rs:510-546`) is `loop { read_message(...); dispatch to
completion; }` - confirmed via grep, zero hits for
`tokio`/`thread`/`async`/`Mutex`/`channel` anywhere in `src/lsp.rs` or
`src/lsp/*.rs`, and the `lsp-server = "0.10.0"` dependency's own
`Connection::stdio()` helper (background I/O threads + a
`Receiver<Message>` supporting `recv_timeout`) is unused (`Cargo.toml:10`,
confirmed no call site). This is the actual prerequisite blocking a naive
"just add a debounce timer" fix - the only thread is parked inside a
blocking stdin read, so there's nowhere to run a timer or answer a
`hover` while a recheck is in flight.

**Fix**: adopt `Connection::stdio()` and restructure `run_loop` around a
channel + `recv_timeout`-based debounce, guarding `Project` access (e.g.
a dedicated worker thread + channel, or `Arc<Mutex<Project>>`) so
`hover`/`definition` can answer against last-good state while a recheck
runs elsewhere. **Effort: medium** - bounded, additive plumbing; does not
touch `update_file`/the registry passes/`transitive_affected_files` at
all (that's separate work, BEYOND_PHPSTAN.md's G1/G2).

### 2.2 Debounce/coalesce rapid didChange bursts, with cancellation of superseded work

Every `didChange` today synchronously runs a full `Project::update_file`
(whole-file re-parse + six whole-registry passes,
`project.rs:305-388,356-363`) and blocks the loop until diagnostics are
published, before the next queued message is even read - a user typing
normally queues every intermediate keystroke state for full, wasteful
rechecking in order, perceived as laggy catch-up rather than instant or
cleanly-batched diagnostics.

**Fix** (needs 2.1 first): buffer only the latest `(uri, text)` per
document behind a short timer (150-300ms); replace-and-restart on a newer
edit for the same URI; add a per-URI generation counter so an
already-completed-but-now-stale recheck's diagnostics get dropped instead
of published. **Effort: large.**

### 2.3 Add `Project::remove_file`

`Project`'s only mutation entry points are `load_file` (new file) and
`update_file` (re-parse existing path) - no `remove_file`/`unload_file`
exists (`project.rs:117-549`, confirmed via grep of every `pub fn`).
There's no way to tell `Project` a file was deleted/moved and unwind its
registry/`dependents` contributions the way `update_file` already unwinds
a file's *old* declarations before re-indexing new ones
(`project.rs:312-314` pattern) - a prerequisite for both 3.1 (file
watching) and correct `didClose` cleanup of scratch buffers.

**Fix**: `Project::remove_file(&mut self, path: &Path) ->
HashMap<PathBuf, Vec<Diagnostic>>`, reusing `update_file`'s existing
unwind logic minus the re-index step, then re-checking transitive
dependents the same way `update_file` does. **Effort: medium.**

### 2.4 Stamp `publishDiagnostics` with the document version

`publish_diagnostics_notification` always sends `version: None`
(`protocol.rs:163-172,169`). Harmless today only because the loop is
fully synchronous (no two rechecks ever in flight). The moment 2.1/2.2
land, a background recheck racing a newer edit has no way to be
recognized as stale by the client without this - diagnostics could
visibly flicker back for text the user already changed.

**Fix**: thread `DidChangeTextDocumentParams.text_document.version`
(already available at the `didChange` call site, `lsp.rs:638-650`)
through to `publish_diagnostics_notification`. Land alongside 2.1/2.2,
not after - it's a co-requisite for them being race-free.
**Effort: small.**

### 2.5 Move the initial workspace scan off `initialize`'s critical path

`load_workspace` runs synchronously inside the `"initialize"` handler
before the response is written (`lsp.rs:559-561,580`) - VSCode's client
blocks on that response, with zero `$/progress` feedback (grep: zero
hits for `WorkDoneProgress`/`ProgressParams` anywhere), indistinguishable
from a hung server for however long the scan takes.

**Fix**: respond to `initialize` immediately, run `load_workspace` (and
1.3's initial diagnostics push) right after or in the background,
reporting `$/progress` once outbound requests exist (3.4).
**Effort: medium.**

---

## Phase 3 — Protocol completeness for a real editor session

### 3.1 `workspace/didChangeWatchedFiles` for external changes

No arm for it anywhere in `handle_notification` (`lsp.rs:627-662`,
falls through the catch-all), and `ServerCapabilities` never advertises
file-watcher interest. A file changed outside the editor - `git
checkout`/`pull`, `composer install` regenerating `vendor/`, another
tool - is invisible to the resident registry until the process restarts.

**Fix**: register a `**/*.php` (+ `composer.{json,lock}`) watcher (via
`client/registerCapability`, needs 3.4 first) gated on the client's
`workspace.didChangeWatchedFiles.dynamicRegistration` capability; handle
Created/Changed via the existing `update_file`/`load_file` path and
Deleted via 2.3's `remove_file`. **Effort: large** (medium once 3.4
exists).

### 3.2 Multi-root workspace: load every folder, handle folder changes

Root resolution takes only `.next()` of `workspace_folders`
(`lsp.rs:551-561`) - every folder past the first is silently dropped,
with no `workspace/didChangeWorkspaceFolders` handling either (zero
grep hits).

**Fix**: call `load_workspace` once per `workspace_folders` entry, and
add a `workspace/didChangeWorkspaceFolders` handler that loads added
folders and invalidates removed ones. **Effort: small-medium.**

### 3.3 `workspace/didChangeConfiguration`

No handling anywhere (zero grep hits) - once 1.1 exists, a user still
has no way to change settings without restarting the server.

**Fix**: re-resolve `Config` and re-run (or incrementally apply to) the
live `Project` on this notification. **Effort: medium**, depends on 1.1.

### 3.4 Outbound-request plumbing

`Message::Response(_) => {}` with the comment "this server never sends a
request of its own" (`lsp.rs:541-543`) - no id allocator, no
pending-request map anywhere. This is the concrete prerequisite for 3.1
(`client/registerCapability`), 2.5 (`window/workDoneProgress/create`),
and pulling `workspace/configuration`.

**Fix**: a monotonic id counter + a small pending-requests map consulted
in the `Message::Response` arm. **Effort: small.**

### 3.5 Position-encoding negotiation (UTF-8)

`lsp.rs`'s own doc comment (lines 16-20) already names the UTF-16-vs-
UTF-8 character-counting mismatch as "an acceptable approximation for
now" - `to_lsp_range`/`from_lsp_position` (`lsp/protocol.rs:139-161`)
count UTF-8 characters while an omitted `position_encoding` capability is
a live protocol claim of UTF-16. On any line with non-ASCII characters
before the position of interest, every diagnostic range/hover/definition
location is silently offset.

**Fix**: read `params.capabilities.general.position_encodings` at
`initialize`; if it contains `"utf-8"` (real `vscode-languageclient`
clients advertise it), set `capabilities.position_encoding =
Some(PositionEncodingKind::UTF8)` in the response - the existing
UTF-8-counting logic then becomes exactly correct instead of an accepted
approximation, no conversion-logic changes needed. **Effort: small.**

### 3.6 Surface failures to the editor, not just stderr

Every recoverable failure (a workspace file that fails to parse, a
malformed inbound message) reports via bare `eprintln!`
(`lsp.rs:159-164,181-190,527-530`) - VSCode's generic client pipes stderr
into an Output channel the user must manually open. A silently-excluded
file (parses to zero diagnostics because it never loaded, not because
it's clean) is a meaningfully different, currently invisible failure
mode.

**Fix**: send `window/logMessage` (or `showMessage` for anything
actionable) for user-relevant failures, at minimum a workspace file that
failed to load. **Effort: small.**

### 3.7 Windows URI drive-letter handling

`uri_to_path`/`path_to_uri` (`lsp/protocol.rs:56-90`) have no special
case for `file:///C:/...`-style Windows URIs (leading `/` before the
drive letter) - would produce a `PathBuf` that doesn't round-trip
correctly through `std::fs` on Windows. Not surfaced today only because
this environment is Linux-primary.

**Fix**: strip the leading `/` before a drive-letter path component in
both directions, with a test exercising a `file:///C:/...` URI even if
only run in CI on a non-Windows host. **Effort: small.**

---

## Phase 4 — The VSCode extension client itself

100% net-new work (a separate TypeScript project) - confirmed absent
everywhere: no `package.json`, no `client/`/`editors/` directory, no
`extension.ts`, no `.vsix`, no `.github` release workflow, anywhere in
this repo outside `vendor/`. Sequence this *after* Phase 1 (nothing to
usefully surface in settings until the server-side config plumbing
exists) but the manifest/client code itself can be written in parallel.

### 4.1 Extension manifest and entry point

`package.json`: `activationEvents: ["onLanguage:php"]`, `main` pointing
at the compiled entry point, `engines.vscode`, `dependencies:
{"vscode-languageclient": "^9"}`. `extension.ts`: `ServerOptions = {
command: <resolved binary path>, args: ["lsp"], transport:
TransportKind.stdio }` (note: `src/lsp.rs`'s own top-of-file doc comment
says "run via `phpstan-rs --lsp`" but the actual clap wiring is a `lsp`
subcommand, `src/main.rs:136,184` - `phpstan-rs lsp`, not a flag; fix
that stale comment while touching this area), `ClientOptions = {
documentSelector: [{scheme: "file", language: "php"}] }`, construct and
`.start()` a `LanguageClient` in `activate()`, `.stop()` in
`deactivate()`. **Effort: small**, standard `vscode-languageclient`
boilerplate.

### 4.2 Binary distribution strategy

No CI/release pipeline exists in this repo at all (`find .github` -
nothing). A JS/TS extension can't compile the Rust server itself.

**Fix**: follow rust-analyzer's known pattern - a CI release job
cross-compiles `phpstan-rs` per target triple (linux-x64/arm64,
darwin-x64/arm64, win32-x64) and uploads GitHub Release assets; the
extension ships thin, with a first-activation download step that detects
`process.platform`/`process.arch`, downloads the matching asset into the
extension's global storage, `chmod +x`s it on POSIX, and caches it. A
`phpstan-rs.serverPath` override setting lets a developer point at a
locally-built binary (also how extension development works before any
release pipeline exists). **Effort: medium.**

### 4.3 User-facing settings

`contributes.configuration` surfacing at minimum: `phpstan-rs.configPath`
(phpstan-rs.toml override), `phpstan-rs.serverPath` (local binary
override, pairs with 4.2), `phpstan-rs.trace.server` (standard
`vscode-languageclient` tracing enum), `phpstan-rs.enable`. Read via
`workspace.getConfiguration("phpstan-rs")` and pass through as CLI args
or `initializationOptions` once 1.1's server-side plumbing exists to
consume them. **Effort: small**, but has nothing to control server-side
until Phase 1 lands.

---

## Suggested sequencing

Phase 1 and Phase 4.1/4.2 (manifest + binary distribution) can start in
parallel - the extension scaffold doesn't need to wait for server-side
fixes to exist as code, only to be *useful*. Phase 2 (concurrency) and
Phase 3 (protocol completeness) are what take the result from "connects
and shows something" to "actually feels real-time and doesn't silently
go stale" - both are meaningfully harder and larger than Phase 1 or 4,
and 2.1 in particular (moving off the single blocking thread) is a real
prerequisite for half of what follows it, not just one more item in the
list.
