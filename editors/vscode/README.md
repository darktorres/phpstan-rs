# phpstan-rs VS Code extension

A thin `vscode-languageclient` wrapper around `phpstan-rs lsp`. Kept as a
separate Node/TypeScript project under `editors/vscode/`, outside the
Cargo workspace - it never touches `Cargo.lock`/`target/`.

## Toolchain

- Package manager: npm (`package-lock.json`, committed).
- Node version: whatever `@types/node`'s major targets (see
  `package.json`) - no `.nvmrc` pinned yet.
- No bundler (esbuild/webpack) yet - deferred; see `VSCODE_EXTENSION_PLAN.md`
  §4.1's own scope notes. The packaged `.vsix` ships `node_modules/`
  as-is for its one runtime dependency, `vscode-languageclient`.

## Development

```sh
npm install
npm run compile   # tsc -> out/
npm run watch     # tsc -w
```

Press F5 in VS Code (with this directory open) to launch an Extension
Development Host.

## Configuration

- `phpstan-rs.enable` (default `true`) - master on/off switch. Disabling
  stops the language server and clears its published diagnostics.
- `phpstan-rs.configPath` (default `null`, per-folder) - path to a
  `phpstan-rs.toml` override, resolved relative to that workspace
  folder's root if not absolute.
- `phpstan-rs.serverPath` (default `null`) - absolute path to a
  `phpstan-rs` binary, validated (exists + executable) before use. Left
  unset, the extension searches `PATH` for a `phpstan-rs` executable
  (`.exe`/`.cmd` first on Windows), then falls back to an automatically
  downloaded and cached binary (see the root `README.md`'s own
  distribution notes).
- `phpstan-rs.trace.server` (`off`/`messages`/`verbose`, default `off`) -
  traces the client/server JSON-RPC traffic in its own output channel;
  needs no extension code, `vscode-languageclient` binds it automatically.
- **`phpstan-rs: Restart Server`** command - re-reads the settings above
  and restarts the language server, applying a changed `configPath`/
  `serverPath`/`enable` without a full "Developer: Reload Window". VS
  Code also offers this automatically (as a notification prompt) after
  changing any of those three settings.

## Tests

- Unit tests (`src/extension.test.ts`, `src/config.test.ts`,
  `src/serverBinary.test.ts`, `src/serverDownload.test.ts` - plain
  `node:test`, no VS Code host needed):

  ```sh
  npm run test:unit
  ```

- Activation smoke test (`src/test/suite/activation.smoke.test.ts`,
  `@vscode/test-electron` - downloads and launches a real VS Code
  instance). Wired into CI (`.github/workflows/extension-ci.yml`'s
  `integration` job, matrixed over Linux/macOS/Windows, `xvfb-run`-wrapped
  on Linux only) but not yet exercised by a real CI run (this repo has
  no GitHub remote yet). Run manually:

  ```sh
  cargo build --release --manifest-path ../../Cargo.toml
  PHPSTAN_RS_TEST_BIN="$(pwd)/../../target/release/phpstan-rs" npm run test:smoke
  ```

  Use a **release** build (`--release`, `target/release/phpstan-rs`),
  not a debug one, for this test: a debug build's cold start (loading
  the embedded PHP stub corpus) was measured taking 30+ seconds in this
  project's own sandboxed dev environment, comfortably past
  `vscode-languageclient`'s own startup patience - a release build's
  equivalent cold start is a small fraction of that.

  **Known, disclosed limitation of this project's own sandboxed dev
  environment**: the second test case (the real server round trip) was
  observed failing here even with a release binary - the language
  server's stdio pipes get torn down immediately when spawned from
  inside the Extension Host (`Unexpected SIGPIPE`/`write EPIPE`), even
  though the identical binary responds correctly both via a direct raw
  stdio round trip and via a plain `child_process.spawn` call from
  ordinary Node outside Electron. This looks specific to nested child-
  process spawning inside this sandbox, not a defect in `extension.ts`'s
  own wiring - the first test case (activation gating, no child process
  involved) passes reliably, as do all unit tests. See the extended
  comment directly above that test in `activation.smoke.test.ts`. Expected
  to pass on a real desktop VS Code install or an actual CI runner
  without this sandbox's specific process-spawning restriction.

`activate()` returns `{ getClientState: () => State | undefined }` (from
`vscode-languageclient`) rather than `void`, specifically so the smoke
test can distinguish "`activate()` has returned" (VS Code's own
`isActive`/`exports` signal) from "the `LanguageClient` has actually
finished spawning and handshaking" (`state === State.Running`) - these
are two different, non-simultaneous events, since `activate()` awaits
resolving/validating the server binary but never awaits `client.start()`
itself.
