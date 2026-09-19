// Deliberately imports no `vscode` module at all - this is what lets
// `extension.test.ts` unit-test `findPhpstanRsOnPath` with plain
// `node:test`, no VS Code host needed. `serverBinary.ts` (4.3) is this
// module's only remaining caller.

import * as fs from "fs";
import * as path from "path";

function isExecutableFile(candidate: string): boolean {
  try {
    const stat = fs.statSync(candidate);
    if (!stat.isFile()) {
      return false;
    }
    fs.accessSync(candidate, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * A `which`/`where`-style PATH search for `phpstan-rs` - on `win32`,
 * tries `.exe`/`.cmd`-suffixed candidates first (a bare extensionless
 * name is not directly executable via `CreateProcess` on Windows the
 * way it is on POSIX), falling back to the bare name last in case a
 * shell wrapper without an extension was placed on PATH deliberately.
 */
export function findPhpstanRsOnPath(
  pathEnv: string | undefined = process.env.PATH,
  platform: NodeJS.Platform = process.platform,
): string | undefined {
  if (!pathEnv) {
    return undefined;
  }
  const candidateNames =
    platform === "win32"
      ? ["phpstan-rs.exe", "phpstan-rs.cmd", "phpstan-rs"]
      : ["phpstan-rs"];
  for (const dir of pathEnv.split(path.delimiter)) {
    if (!dir) {
      continue;
    }
    for (const name of candidateNames) {
      const candidate = path.join(dir, name);
      if (isExecutableFile(candidate)) {
        return candidate;
      }
    }
  }
  return undefined;
}
