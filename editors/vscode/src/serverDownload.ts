// Deliberately imports no `vscode` module - kept unit-testable with
// plain node:test, matching `serverResolution.ts`'s own convention.
// `extension.ts` (which does import `vscode`) extracts
// `context.globalStorageUri.fsPath` and calls `resolveServerPath` here.

import * as cp from "child_process";
import * as crypto from "crypto";
import * as fs from "fs";
import * as path from "path";
import * as zlib from "zlib";

// UNSET until this project actually has a GitHub remote (confirmed
// absent at the time this file was written - see VSCODE_EXTENSION_PLAN.md
// §4.2's own disclosed prerequisite). Every real download call fails
// fast with an actionable error until this is filled in, rather than
// silently trying (and failing) to reach a placeholder URL. Tests
// override it via `ResolveServerPathOptions.githubRepo`.
const GITHUB_REPO = "";

const MIN_PLAUSIBLE_BINARY_SIZE_BYTES = 1024;
const DEFAULT_LOCK_STALE_MS = 2 * 60 * 1000;
const DEFAULT_LOCK_POLL_MS = 150;
const DEFAULT_LOCK_POLL_TIMEOUT_MS = 5 * 60 * 1000;

/** One of the 5 published (platform, arch) pairs, or `undefined` for an
 * unsupported combination - `resolveServerPath` surfaces that as one
 * explicit error rather than attempting a download that could never
 * succeed. */
export function resolveServerTriple(
  platform: NodeJS.Platform = process.platform,
  arch: string = process.arch,
): string | undefined {
  const table: Record<string, string> = {
    "linux-x64": "x86_64-unknown-linux-gnu",
    "linux-arm64": "aarch64-unknown-linux-gnu",
    "darwin-arm64": "aarch64-apple-darwin",
    "darwin-x64": "x86_64-apple-darwin",
    "win32-x64": "x86_64-pc-windows-msvc",
  };
  return table[`${platform}-${arch}`];
}

/** The extension and the server binary are versioned in lockstep off one
 * release tag `v<semver>` - `package.json`'s own `version` field IS that
 * semver. */
export function getExpectedServerVersion(): string {
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const pkg = require("../package.json") as { version: string };
  return pkg.version;
}

interface InstalledSidecar {
  version: string;
  triple: string;
}

/** Everything about `resolveServerPath` a test needs to control that a
 * real caller never does - real callers (`extension.ts`) pass none of
 * these and get the real platform/arch/version/network/timings. */
export interface ResolveServerPathOptions {
  platform?: NodeJS.Platform;
  arch?: string;
  version?: string;
  githubRepo?: string;
  fetchImpl?: typeof fetch;
  onTempPath?: (tmpPath: string) => void;
  lockPollMs?: number;
  lockStaleMs?: number;
  lockPollTimeoutMs?: number;
}

interface ResolvedOptions {
  platform: NodeJS.Platform;
  arch: string;
  version: string;
  githubRepo: string;
  fetchImpl: typeof fetch;
  onTempPath: (tmpPath: string) => void;
  lockPollMs: number;
  lockStaleMs: number;
  lockPollTimeoutMs: number;
}

function resolveOptions(options: ResolveServerPathOptions): ResolvedOptions {
  return {
    platform: options.platform ?? process.platform,
    arch: options.arch ?? process.arch,
    version: options.version ?? getExpectedServerVersion(),
    githubRepo: options.githubRepo ?? GITHUB_REPO,
    fetchImpl: options.fetchImpl ?? fetch,
    onTempPath: options.onTempPath ?? (() => {}),
    lockPollMs: options.lockPollMs ?? DEFAULT_LOCK_POLL_MS,
    lockStaleMs: options.lockStaleMs ?? DEFAULT_LOCK_STALE_MS,
    lockPollTimeoutMs: options.lockPollTimeoutMs ?? DEFAULT_LOCK_POLL_TIMEOUT_MS,
  };
}

function serverFileBaseName(
  triple: string,
  version: string,
  platform: NodeJS.Platform,
): string {
  const base = `phpstan-rs-${triple}-${version}`;
  return platform === "win32" ? `${base}.exe` : base;
}

function downloadAssetName(triple: string, platform: NodeJS.Platform): string {
  const base = `phpstan-rs-${triple}`;
  return platform === "win32" ? `${base}.exe` : base;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function readSidecar(sidecarPath: string): InstalledSidecar | undefined {
  try {
    return JSON.parse(fs.readFileSync(sidecarPath, "utf8")) as InstalledSidecar;
  } catch {
    return undefined;
  }
}

function isValidCachedInstall(
  finalPath: string,
  sidecarPath: string,
  expectedVersion: string,
  expectedTriple: string,
): boolean {
  let stat: fs.Stats;
  try {
    stat = fs.statSync(finalPath);
  } catch {
    return false;
  }
  if (!stat.isFile() || stat.size < MIN_PLAUSIBLE_BINARY_SIZE_BYTES) {
    return false;
  }
  const sidecar = readSidecar(sidecarPath);
  return (
    sidecar !== undefined &&
    sidecar.version === expectedVersion &&
    sidecar.triple === expectedTriple
  );
}

/** `fs.open(path, 'wx')` - fails with `EEXIST` if another process/window
 * already holds the lock, giving two racing first-activations a real,
 * atomic single-flight point rather than both downloading independently. */
function tryAcquireLock(lockPath: string): boolean {
  try {
    const fd = fs.openSync(lockPath, "wx");
    fs.writeSync(fd, String(process.pid));
    fs.closeSync(fd);
    return true;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "EEXIST") {
      return false;
    }
    throw err;
  }
}

function releaseLock(lockPath: string): void {
  try {
    fs.unlinkSync(lockPath);
  } catch {
    // Already gone - fine, releasing is best-effort.
  }
}

function isLockStale(lockPath: string, lockStaleMs: number): boolean {
  try {
    const stat = fs.statSync(lockPath);
    return Date.now() - stat.mtimeMs > lockStaleMs;
  } catch {
    // Lock disappeared between the existence check and this stat -
    // treat as "not stale", the caller's own loop will just retry.
    return false;
  }
}

async function fetchBinary(fetchImpl: typeof fetch, url: string): Promise<Buffer> {
  const res = await fetchImpl(url);
  if (!res.ok) {
    throw new Error(`phpstan-rs: failed to download ${url} (HTTP ${res.status}).`);
  }
  return Buffer.from(await res.arrayBuffer());
}

async function fetchText(fetchImpl: typeof fetch, url: string): Promise<string> {
  const res = await fetchImpl(url);
  if (!res.ok) {
    throw new Error(`phpstan-rs: failed to download ${url} (HTTP ${res.status}).`);
  }
  return res.text();
}

/** Downloads to a PER-ATTEMPT-UNIQUE temp path (never a shared one - two
 * concurrent writers to the same temp path would interleave and corrupt
 * it; atomic rename alone only protects the FINAL path from ever
 * showing a half-written file), verifies its sha256, decompresses,
 * chmods on POSIX, strips the macOS quarantine xattr, then atomically
 * renames into `finalPath`. Always removes its own unique tmp file
 * (rename succeeded, so there's nothing left to remove; or an error was
 * thrown, so the half-downloaded file must not linger) via `finally`. */
async function performDownload(
  finalPath: string,
  sidecarPath: string,
  triple: string,
  opts: ResolvedOptions,
): Promise<void> {
  if (!opts.githubRepo) {
    throw new Error(
      "phpstan-rs: no download source configured (GITHUB_REPO is unset - " +
        "this project has no GitHub remote yet). Set phpstan-rs.serverPath instead.",
    );
  }
  const assetName = downloadAssetName(triple, opts.platform);
  const baseUrl = `https://github.com/${opts.githubRepo}/releases/download/v${opts.version}`;
  const tmpPath = `${finalPath}.tmp-${crypto.randomBytes(8).toString("hex")}`;
  opts.onTempPath(tmpPath);
  try {
    const [gzBytes, sha256Line] = await Promise.all([
      fetchBinary(opts.fetchImpl, `${baseUrl}/${assetName}.gz`),
      fetchText(opts.fetchImpl, `${baseUrl}/${assetName}.gz.sha256`),
    ]);
    const expectedSha256 = sha256Line.trim().split(/\s+/)[0];
    const actualSha256 = crypto.createHash("sha256").update(gzBytes).digest("hex");
    if (actualSha256 !== expectedSha256) {
      throw new Error(
        `phpstan-rs: checksum mismatch downloading ${assetName} ` +
          `(expected ${expectedSha256}, got ${actualSha256}).`,
      );
    }
    const decompressed = zlib.gunzipSync(gzBytes);
    fs.writeFileSync(tmpPath, decompressed);
    if (opts.platform !== "win32") {
      fs.chmodSync(tmpPath, 0o755);
    }
    if (opts.platform === "darwin") {
      try {
        cp.execFileSync("xattr", ["-dr", "com.apple.quarantine", tmpPath]);
      } catch {
        // `xattr` missing, or nothing to strip - not fatal either way.
      }
    }
    fs.renameSync(tmpPath, finalPath);
    fs.writeFileSync(
      sidecarPath,
      JSON.stringify({ version: opts.version, triple } satisfies InstalledSidecar),
    );
  } finally {
    try {
      fs.unlinkSync(tmpPath);
    } catch {
      // Either already renamed away (the success path), or never
      // created (failed before the write) - both fine to ignore.
    }
  }
}

async function waitForInstall(
  finalPath: string,
  sidecarPath: string,
  lockPath: string,
  triple: string,
  opts: ResolvedOptions,
): Promise<string> {
  const deadline = Date.now() + opts.lockPollTimeoutMs;
  while (Date.now() < deadline) {
    if (isValidCachedInstall(finalPath, sidecarPath, opts.version, triple)) {
      return finalPath;
    }
    if (!fs.existsSync(lockPath)) {
      // The lock holder finished (but produced no valid install, per
      // the check above - it must have failed) or crashed without
      // cleaning up - either way, try to become the new owner.
      if (tryAcquireLock(lockPath)) {
        try {
          await performDownload(finalPath, sidecarPath, triple, opts);
          return finalPath;
        } finally {
          releaseLock(lockPath);
        }
      }
      // Lost the race to acquire it to yet another caller - keep polling.
    } else if (isLockStale(lockPath, opts.lockStaleMs)) {
      // A crashed process that never reached its own `finally` - reclaim.
      releaseLock(lockPath);
    }
    await sleep(opts.lockPollMs);
  }
  throw new Error(
    "phpstan-rs: timed out waiting for another window to finish downloading the server binary.",
  );
}

/** Order: (1) `configuredPath` wins verbatim, touching neither global
 * storage nor the network; (2) a cached, version-and-triple-matched,
 * plausibly-sized binary in `globalStorageDir/server/`; (3) download-
 * and-install via the lock-coordinated single-flight procedure above. */
export async function resolveServerPath(
  configuredPath: string | undefined,
  globalStorageDir: string,
  options: ResolveServerPathOptions = {},
): Promise<string> {
  if (configuredPath && configuredPath.trim().length > 0) {
    return configuredPath;
  }

  const opts = resolveOptions(options);
  const triple = resolveServerTriple(opts.platform, opts.arch);
  if (!triple) {
    throw new Error(
      `phpstan-rs: no published server binary for ${opts.platform}/${opts.arch}. ` +
        "Set phpstan-rs.serverPath to a binary you built or obtained yourself.",
    );
  }

  const serverDir = path.join(globalStorageDir, "server");
  fs.mkdirSync(serverDir, { recursive: true });
  const finalPath = path.join(serverDir, serverFileBaseName(triple, opts.version, opts.platform));
  const sidecarPath = `${finalPath}.installed.json`;

  if (isValidCachedInstall(finalPath, sidecarPath, opts.version, triple)) {
    return finalPath;
  }

  const lockPath = `${finalPath}.lock`;
  if (tryAcquireLock(lockPath)) {
    try {
      await performDownload(finalPath, sidecarPath, triple, opts);
      return finalPath;
    } finally {
      releaseLock(lockPath);
    }
  }
  return waitForInstall(finalPath, sidecarPath, lockPath, triple, opts);
}
