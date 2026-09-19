import * as path from "path";
import Mocha = require("mocha");
import { globSync } from "glob";

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
