import * as path from "path";
import { runTests } from "@vscode/test-electron";

/**
 * Launches a real VS Code instance (downloaded/cached by
 * `@vscode/test-electron`) with this extension installed, running the
 * Mocha suite in `./suite/index.js` inside it. Not wired into CI (this
 * repo has none today) - run manually via `npm run test:smoke` inside
 * `editors/vscode/`. Requires a built `phpstan-rs` binary; point
 * `PHPSTAN_RS_TEST_BIN` at one (see `suite/activation.smoke.test.ts`).
 */
async function main(): Promise<void> {
  const extensionDevelopmentPath = path.resolve(__dirname, "../../");
  const extensionTestsPath = path.resolve(__dirname, "./suite/index");

  await runTests({
    extensionDevelopmentPath,
    extensionTestsPath,
    // `--no-sandbox`: standard flag for running an Electron-based test
    // host inside a container/CI environment lacking the kernel support
    // Electron's own Chromium sandbox needs. Kept as a reasonable
    // default even though it did NOT resolve the SIGPIPE/EPIPE issue
    // documented on the second test case below - that issue appears to
    // be specific to spawning a child process from within THIS sandbox's
    // own nested Extension Host process tree, not the Chromium sandbox.
    launchArgs: ["--no-sandbox"],
  });
}

main().catch((err) => {
  console.error("Failed to run smoke tests:", err);
  process.exit(1);
});
