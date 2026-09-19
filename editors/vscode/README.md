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

- `phpstan-rs.serverPath` - absolute path to a `phpstan-rs` binary.
  Left empty, the extension searches `PATH` for a `phpstan-rs`
  executable (`.exe`/`.cmd` first on Windows).

## Tests

- Unit tests (`src/extension.test.ts`, plain `node:test`, no VS Code
  host needed):

  ```sh
  npm run test:unit
  ```

- Activation smoke test (`src/test/suite/activation.smoke.test.ts`,
  `@vscode/test-electron` - downloads and launches a real VS Code
  instance). Not wired into CI (this repo has none yet) - run manually:

  ```sh
  cargo build --release --manifest-path ../../Cargo.toml
  PHPSTAN_RS_TEST_BIN="$(pwd)/../../target/debug/phpstan-rs" npm run test:smoke
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
  involved) passes reliably, as do all 10 unit tests. See the extended
  comment directly above that test in `activation.smoke.test.ts`.
