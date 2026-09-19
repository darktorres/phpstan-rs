import { test } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { validateServerPath, resolveServerBinary } from "./serverBinary";
import { Settings } from "./config";

function settings(overrides: Partial<Settings> = {}): Settings {
  return { enable: true, configPath: null, serverPath: null, ...overrides };
}

function withTempDir<T>(fn: (dir: string) => T): T {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "phpstan-rs-vscode-serverbinary-test-"));
  try {
    return fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test("validateServerPath throws for a missing file", () => {
  withTempDir((dir) => {
    assert.throws(() => validateServerPath(path.join(dir, "nope")), /missing file/);
  });
});

test("validateServerPath throws when the path is a directory, not a file", () => {
  withTempDir((dir) => {
    assert.throws(() => validateServerPath(dir), /does not point to a file/);
  });
});

test("validateServerPath throws on POSIX for a file lacking the execute bit", () => {
  withTempDir((dir) => {
    const file = path.join(dir, "phpstan-rs");
    fs.writeFileSync(file, "not executable");
    fs.chmodSync(file, 0o644);
    assert.throws(() => validateServerPath(file, "linux"), /non-executable/);
  });
});

test("validateServerPath accepts an executable file on POSIX", () => {
  withTempDir((dir) => {
    const file = path.join(dir, "phpstan-rs");
    fs.writeFileSync(file, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(file, 0o755);
    assert.doesNotThrow(() => validateServerPath(file, "linux"));
  });
});

test("validateServerPath rejects a file with no recognized executable extension on win32", () => {
  withTempDir((dir) => {
    const file = path.join(dir, "phpstan-rs");
    fs.writeFileSync(file, "not a real PE");
    assert.throws(() => validateServerPath(file, "win32"), /does not look executable/);
  });
});

test("validateServerPath accepts a .exe file on win32 regardless of the POSIX execute bit", () => {
  withTempDir((dir) => {
    const file = path.join(dir, "phpstan-rs.exe");
    fs.writeFileSync(file, "not a real PE, just a test fixture");
    assert.doesNotThrow(() => validateServerPath(file, "win32"));
  });
});

test("resolveServerBinary returns a valid configured serverPath without consulting PATH or downloading", async () => {
  await withTempDir(async (dir) => {
    const file = path.join(dir, "phpstan-rs");
    fs.writeFileSync(file, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(file, 0o755);
    const result = await resolveServerBinary(
      settings({ serverPath: file }),
      path.join(dir, "global-storage"),
      "linux",
      "/should/not/be/searched",
    );
    assert.equal(result, file);
    assert.equal(fs.existsSync(path.join(dir, "global-storage")), false);
  });
});

test("resolveServerBinary rejects when the configured serverPath is invalid", async () => {
  await withTempDir(async (dir) => {
    await assert.rejects(
      () =>
        resolveServerBinary(
          settings({ serverPath: path.join(dir, "does-not-exist") }),
          path.join(dir, "global-storage"),
          "linux",
          undefined,
        ),
      /missing file/,
    );
  });
});

test("resolveServerBinary falls back to a PATH match when serverPath is unset", async () => {
  await withTempDir(async (dir) => {
    const file = path.join(dir, "phpstan-rs");
    fs.writeFileSync(file, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(file, 0o755);
    const result = await resolveServerBinary(settings(), path.join(dir, "global-storage"), "linux", dir);
    assert.equal(result, file);
    assert.equal(fs.existsSync(path.join(dir, "global-storage")), false);
  });
});

test("resolveServerBinary defers to 4.2's download pipeline when neither serverPath nor PATH resolve anything", async () => {
  await withTempDir(async (dir) => {
    // No GITHUB_REPO is configured in the shipped source (this repo has
    // no GitHub remote yet) - `resolveServerPath` fails fast with its
    // own specific error, which is exactly what proves delegation
    // happened here rather than resolveServerBinary silently returning
    // something bogus.
    await assert.rejects(
      () => resolveServerBinary(settings(), path.join(dir, "global-storage"), "linux", ""),
      /no download source configured/,
    );
  });
});
