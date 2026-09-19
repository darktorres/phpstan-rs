import { test } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { findPhpstanRsOnPath } from "./serverResolution";

/** A fresh temp directory containing a real, executable file named
 * `name` - lets `findPhpstanRsOnPath` tests exercise the real
 * `fs.statSync`/`fs.accessSync` filesystem checks against a genuine
 * file, not just directories that don't exist at all. */
function withExecutable(name: string): { dir: string; cleanup: () => void } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "phpstan-rs-vscode-test-"));
  fs.writeFileSync(path.join(dir, name), "#!/bin/sh\nexit 0\n");
  fs.chmodSync(path.join(dir, name), 0o755);
  return { dir, cleanup: () => fs.rmSync(dir, { recursive: true, force: true }) };
}

test("findPhpstanRsOnPath prefers phpstan-rs.exe over a same-directory bare name on win32", () => {
  // Both a bare `phpstan-rs` and a `phpstan-rs.exe` exist in the same
  // directory - the .exe-suffixed candidate must win, since it's tried
  // first per the win32 candidate order. This is exercised at the
  // logic level only, on this (POSIX) environment's own filesystem -
  // there is no Windows runner here to validate real `CreateProcess`
  // extension resolution end-to-end (see this item's own open-risks
  // note).
  const { dir, cleanup } = withExecutable("phpstan-rs");
  try {
    fs.writeFileSync(path.join(dir, "phpstan-rs.exe"), "not a real PE, just a test fixture\n");
    fs.chmodSync(path.join(dir, "phpstan-rs.exe"), 0o755);
    const result = findPhpstanRsOnPath(dir, "win32");
    assert.equal(result, path.join(dir, "phpstan-rs.exe"));
  } finally {
    cleanup();
  }
});

test("findPhpstanRsOnPath returns undefined for an empty PATH", () => {
  const result = findPhpstanRsOnPath("", "linux");
  assert.equal(result, undefined);
});

test("findPhpstanRsOnPath returns undefined for an undefined PATH", () => {
  const result = findPhpstanRsOnPath(undefined, "linux");
  assert.equal(result, undefined);
});

test("findPhpstanRsOnPath finds a real executable named phpstan-rs on POSIX", () => {
  const { dir, cleanup } = withExecutable("phpstan-rs");
  try {
    const result = findPhpstanRsOnPath(dir, "linux");
    assert.equal(result, path.join(dir, "phpstan-rs"));
  } finally {
    cleanup();
  }
});

test("findPhpstanRsOnPath finds a real phpstan-rs.exe on win32", () => {
  const { dir, cleanup } = withExecutable("phpstan-rs.exe");
  try {
    const result = findPhpstanRsOnPath(dir, "win32");
    assert.equal(result, path.join(dir, "phpstan-rs.exe"));
  } finally {
    cleanup();
  }
});

test("findPhpstanRsOnPath searches every PATH directory in order until a match", () => {
  const empty = fs.mkdtempSync(path.join(os.tmpdir(), "phpstan-rs-vscode-test-"));
  const { dir, cleanup } = withExecutable("phpstan-rs");
  try {
    const result = findPhpstanRsOnPath([empty, dir].join(path.delimiter), "linux");
    assert.equal(result, path.join(dir, "phpstan-rs"));
  } finally {
    cleanup();
    fs.rmSync(empty, { recursive: true, force: true });
  }
});
