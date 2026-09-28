import * as path from "path";
import MochaModule = require("mocha");
import { globSync } from "glob";

// mocha is an ES module whose default export is the constructor. Node's
// `require` of an ES module returns the constructor itself on newer Node
// and the module namespace on the older Node that VS Code's extension host
// runs, so take `default` when it is there.
const Mocha: typeof MochaModule =
  (MochaModule as unknown as { default?: typeof MochaModule }).default ?? MochaModule;

/** Loaded inside the real VS Code test host by `@vscode/test-electron`
 * (see `../runTest.ts`) - discovers and runs every `*.test.js` file
 * under this directory via Mocha's own BDD interface. */
export function run(): Promise<void> {
  const mocha = new Mocha({ ui: "bdd", timeout: 30_000 });
  const testsRoot = __dirname;

  return new Promise((resolve, reject) => {
    const files = globSync("**/*.test.js", { cwd: testsRoot });
    for (const file of files) {
      mocha.addFile(path.resolve(testsRoot, file));
    }
    try {
      mocha.run((failures) => {
        if (failures > 0) {
          reject(new Error(`${failures} smoke test(s) failed.`));
        } else {
          resolve();
        }
      });
    } catch (err) {
      reject(err);
    }
  });
}
