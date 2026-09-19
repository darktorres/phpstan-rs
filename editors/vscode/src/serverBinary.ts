// Uses only fs/path/child_process - no `vscode` import, so it has no
// test-loading issue (matches config.ts/serverDownload.ts/
// serverResolution.ts's own convention).

import * as fs from "fs";
import * as path from "path";
import { Settings } from "./config";
import { findPhpstanRsOnPath } from "./serverResolution";
import { resolveServerPath } from "./serverDownload";

function isWindowsExecutableExtension(candidate: string): boolean {
  const ext = path.extname(candidate).toLowerCase();
  return ext === ".exe" || ext === ".cmd" || ext === ".bat";
}

/** Throws a specific, actionable error rather than letting
 * `vscode-languageclient`'s own spawn failure surface as an opaque
 * "client failed to start". POSIX: existence + the execute bit.
 * Windows: no execute-bit concept exists, so existence plus a
 * recognized executable extension stands in. */
export function validateServerPath(
  candidate: string,
  platform: NodeJS.Platform = process.platform,
): void {
  let stat: fs.Stats;
  try {
    stat = fs.statSync(candidate);
  } catch {
    throw new Error(`phpstan-rs.serverPath points to a missing file: ${candidate}`);
  }
  if (!stat.isFile()) {
    throw new Error(`phpstan-rs.serverPath does not point to a file: ${candidate}`);
  }
  if (platform === "win32") {
    if (!isWindowsExecutableExtension(candidate)) {
      throw new Error(
        "phpstan-rs.serverPath does not look executable " +
          `(expected a .exe/.cmd/.bat file): ${candidate}`,
      );
    }
    return;
  }
  try {
    fs.accessSync(candidate, fs.constants.X_OK);
  } catch {
    throw new Error(`phpstan-rs.serverPath points to a non-executable file: ${candidate}`);
  }
}

/** Order: (1) `settings.serverPath`, validated (a set-but-broken
 * override fails loudly rather than silently falling through to a
 * download); (2) an existing on-PATH install (4.1) - so a `cargo
 * install`/distro package a user already has is never shadowed by an
 * auto-download; (3) 4.2's cached-binary/download pipeline. */
export async function resolveServerBinary(
  settings: Settings,
  globalStorageDir: string,
  platform: NodeJS.Platform = process.platform,
  pathEnv: string | undefined = process.env.PATH,
): Promise<string> {
  if (settings.serverPath) {
    validateServerPath(settings.serverPath, platform);
    return settings.serverPath;
  }
  const onPath = findPhpstanRsOnPath(pathEnv, platform);
  if (onPath) {
    return onPath;
  }
  return resolveServerPath(undefined, globalStorageDir, { platform });
}
