import { test } from "node:test";
import * as assert from "node:assert/strict";
import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as zlib from "node:zlib";
import {
  resolveServerTriple,
  getExpectedServerVersion,
  resolveServerPath,
} from "./serverDownload";

function tmpDir(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "phpstan-rs-vscode-download-test-"));
}

/** A minimal `Response`-shaped object - enough of the `fetch` contract
 * for `serverDownload.ts`'s own `fetchBinary`/`fetchText` to consume,
 * without pulling in a real HTTP mocking library for what is, in the
 * end, two methods (`.ok`/`.status`, `.arrayBuffer()`/`.text()`). */
function okResponse(body: Buffer | string): Response {
  const buf = typeof body === "string" ? Buffer.from(body) : body;
  return {
    ok: true,
    status: 200,
    arrayBuffer: async () => buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength),
    text: async () => buf.toString("utf8"),
  } as unknown as Response;
}

function notFoundResponse(): Response {
  return { ok: false, status: 404 } as unknown as Response;
}

/** A fake release asset set: a gzip-compressed "binary" of plausible
 * size plus its real sha256 - `fetchImpl` below serves these for any
 * `.gz`/`.gz.sha256` URL, so tests don't depend on `GITHUB_REPO` (which
 * is intentionally unset in the shipped source) or real network access. */
function fakeRelease(contents = "#!/bin/sh\necho fake-phpstan-rs\n".repeat(64)) {
  const raw = Buffer.from(contents);
  const gz = zlib.gzipSync(raw);
  const sha256 = crypto.createHash("sha256").update(gz).digest("hex");
  return { gz, sha256 };
}

function fetchImplFor(release: { gz: Buffer; sha256: string }, opts: { badSha256?: boolean; failAll?: boolean } = {}) {
  const calls: string[] = [];
  const fetchImpl = (async (url: string | URL) => {
    calls.push(String(url));
    if (opts.failAll) {
      return notFoundResponse();
    }
    if (String(url).endsWith(".sha256")) {
      return okResponse(opts.badSha256 ? "0".repeat(64) : release.sha256);
    }
    return okResponse(release.gz);
  }) as unknown as typeof fetch;
  return { fetchImpl, calls };
}

test("resolveServerTriple maps all 5 published platform/arch pairs", () => {
  assert.equal(resolveServerTriple("linux", "x64"), "x86_64-unknown-linux-gnu");
  assert.equal(resolveServerTriple("linux", "arm64"), "aarch64-unknown-linux-gnu");
  assert.equal(resolveServerTriple("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(resolveServerTriple("darwin", "x64"), "x86_64-apple-darwin");
  assert.equal(resolveServerTriple("win32", "x64"), "x86_64-pc-windows-msvc");
});

test("resolveServerTriple returns undefined for an unsupported combination", () => {
  assert.equal(resolveServerTriple("linux", "ia32"), undefined);
  assert.equal(resolveServerTriple("freebsd", "x64"), undefined);
});

test("getExpectedServerVersion reads package.json's own version field", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"));
  assert.equal(getExpectedServerVersion(), pkg.version);
});

test("an unsupported platform/arch rejects without attempting any download", async () => {
  const dir = tmpDir();
  const { fetchImpl, calls } = fetchImplFor(fakeRelease());
  try {
    await assert.rejects(
      () =>
        resolveServerPath(undefined, dir, {
          platform: "freebsd" as NodeJS.Platform,
          arch: "x64",
          fetchImpl,
        }),
      /no published server binary for freebsd\/x64/,
    );
    assert.equal(calls.length, 0);
    assert.deepEqual(fs.readdirSync(dir), []);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a configured serverPath short-circuits without touching global storage or the network", async () => {
  const dir = tmpDir();
  // A directory that doesn't exist - if `resolveServerPath` ever tried
  // to touch global storage before checking `configuredPath`, creating
  // `server/` under it would throw ENOENT and fail this test.
  const untouchedGlobalStorage = path.join(dir, "does-not-exist");
  const { fetchImpl, calls } = fetchImplFor(fakeRelease());
  try {
    const result = await resolveServerPath("/opt/phpstan-rs/phpstan-rs", untouchedGlobalStorage, {
      fetchImpl,
    });
    assert.equal(result, "/opt/phpstan-rs/phpstan-rs");
    assert.equal(calls.length, 0);
    assert.equal(fs.existsSync(untouchedGlobalStorage), false);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("downloads, verifies, and installs on a cold cache, then reuses the cache on a second call", async () => {
  const dir = tmpDir();
  const release = fakeRelease();
  const { fetchImpl, calls } = fetchImplFor(release);
  try {
    const first = await resolveServerPath(undefined, dir, {
      platform: "linux",
      arch: "x64",
      version: "1.2.3",
      githubRepo: "example/phpstan-rs",
      fetchImpl,
    });
    assert.match(first, /phpstan-rs-x86_64-unknown-linux-gnu-1\.2\.3$/);
    assert.equal(fs.statSync(first).mode & 0o777, 0o755);
    const sidecar = JSON.parse(fs.readFileSync(`${first}.installed.json`, "utf8"));
    assert.deepEqual(sidecar, { version: "1.2.3", triple: "x86_64-unknown-linux-gnu" });
    assert.equal(calls.length, 2);

    const second = await resolveServerPath(undefined, dir, {
      platform: "linux",
      arch: "x64",
      version: "1.2.3",
      githubRepo: "example/phpstan-rs",
      fetchImpl,
    });
    assert.equal(second, first);
    // No new fetches - the cache hit must not touch the network at all.
    assert.equal(calls.length, 2);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a version bump (simulated extension upgrade) re-downloads instead of reusing the old cache", async () => {
  const dir = tmpDir();
  const release = fakeRelease();
  const { fetchImpl, calls } = fetchImplFor(release);
  try {
    const v1 = await resolveServerPath(undefined, dir, {
      platform: "linux",
      arch: "x64",
      version: "1.0.0",
      githubRepo: "example/phpstan-rs",
      fetchImpl,
    });
    const v2 = await resolveServerPath(undefined, dir, {
      platform: "linux",
      arch: "x64",
      version: "2.0.0",
      githubRepo: "example/phpstan-rs",
      fetchImpl,
    });
    assert.notEqual(v1, v2);
    assert.ok(fs.existsSync(v2));
    assert.equal(calls.length, 4);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a sha256 mismatch rejects, releases the lock, and never chmods/renames a tmp file into place", async () => {
  const dir = tmpDir();
  const release = fakeRelease();
  const { fetchImpl } = fetchImplFor(release, { badSha256: true });
  const tempPaths: string[] = [];
  try {
    await assert.rejects(
      () =>
        resolveServerPath(undefined, dir, {
          platform: "linux",
          arch: "x64",
          version: "1.2.3",
          githubRepo: "example/phpstan-rs",
          fetchImpl,
          onTempPath: (p) => tempPaths.push(p),
        }),
      /checksum mismatch/,
    );
    assert.equal(tempPaths.length, 1);
    const serverDir = path.join(dir, "server");
    const remaining = fs.readdirSync(serverDir);
    // Neither the final binary, its sidecar, the lock, nor the tmp file
    // may still exist - a failed verification must leave nothing behind.
    assert.deepEqual(remaining, []);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a download failure still releases the lock via finally, leaving no lock file behind", async () => {
  const dir = tmpDir();
  const { fetchImpl } = fetchImplFor(fakeRelease(), { failAll: true });
  try {
    await assert.rejects(
      () =>
        resolveServerPath(undefined, dir, {
          platform: "linux",
          arch: "x64",
          version: "1.2.3",
          githubRepo: "example/phpstan-rs",
          fetchImpl,
        }),
      /HTTP 404/,
    );
    const serverDir = path.join(dir, "server");
    const remaining = fs.readdirSync(serverDir);
    assert.deepEqual(remaining, []);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("two racing resolveServerPath calls single-flight through one real download and agree on the final path", async () => {
  const dir = tmpDir();
  const release = fakeRelease();
  let binaryFetches = 0;
  const tempPaths: string[] = [];
  // The winner's binary fetch is deliberately slow, so the loser is
  // guaranteed to observe the lock file before the winner finishes -
  // without this, a fast enough machine could let both callers race
  // past `tryAcquireLock` before either lock file exists on disk.
  const fetchImpl = (async (url: string | URL) => {
    if (String(url).endsWith(".sha256")) {
      return okResponse(release.sha256);
    }
    binaryFetches += 1;
    await new Promise((resolve) => setTimeout(resolve, 100));
    return okResponse(release.gz);
  }) as unknown as typeof fetch;

  try {
    const commonOptions = {
      platform: "linux" as NodeJS.Platform,
      arch: "x64",
      version: "1.2.3",
      githubRepo: "example/phpstan-rs",
      fetchImpl,
      onTempPath: (p: string) => tempPaths.push(p),
      lockPollMs: 10,
    };
    const [a, b] = await Promise.all([
      resolveServerPath(undefined, dir, commonOptions),
      resolveServerPath(undefined, dir, commonOptions),
    ]);
    assert.equal(a, b);
    assert.equal(binaryFetches, 1);
    assert.equal(tempPaths.length, 1);
    assert.ok(fs.existsSync(a));
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a stale lock left by a crashed process is reclaimed instead of waited on forever", async () => {
  const dir = tmpDir();
  const release = fakeRelease();
  const { fetchImpl } = fetchImplFor(release);
  const serverDir = path.join(dir, "server");
  fs.mkdirSync(serverDir, { recursive: true });
  const finalPath = path.join(serverDir, "phpstan-rs-x86_64-unknown-linux-gnu-1.2.3");
  const lockPath = `${finalPath}.lock`;
  fs.writeFileSync(lockPath, "99999999");
  // Backdate the lock well past the test's own (short) staleness
  // threshold, simulating a process that acquired it and then crashed
  // before its own `finally` could release it.
  const old = new Date(Date.now() - 10_000);
  fs.utimesSync(lockPath, old, old);

  try {
    const result = await resolveServerPath(undefined, dir, {
      platform: "linux",
      arch: "x64",
      version: "1.2.3",
      githubRepo: "example/phpstan-rs",
      fetchImpl,
      lockPollMs: 10,
      lockStaleMs: 50,
    });
    assert.equal(result, finalPath);
    assert.ok(fs.existsSync(finalPath));
    assert.equal(fs.existsSync(lockPath), false);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
