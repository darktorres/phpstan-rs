import { test } from "node:test";
import * as assert from "node:assert/strict";
import { resolveConfigPath, buildInitializationOptions, Settings } from "./config";

function settings(overrides: Partial<Settings> = {}): Settings {
  return { enable: true, configPath: null, serverPath: null, ...overrides };
}

test("resolveConfigPath resolves a relative path against the open folder", () => {
  const result = resolveConfigPath(settings({ configPath: "rel/path.toml" }), "/ws/folder");
  assert.deepEqual(result, { path: "/ws/folder/rel/path.toml" });
});

test("resolveConfigPath returns an absolute path unchanged, even with no folder open", () => {
  const result = resolveConfigPath(settings({ configPath: "/abs/path.toml" }), undefined);
  assert.deepEqual(result, { path: "/abs/path.toml" });
});

test("resolveConfigPath returns the raw value plus a warning when relative with no folder open", () => {
  const result = resolveConfigPath(settings({ configPath: "rel/path.toml" }), undefined);
  assert.equal(result.path, "rel/path.toml");
  assert.equal(typeof result.warning, "string");
  assert.ok(result.warning && result.warning.length > 0);
});

test("resolveConfigPath returns no path and no warning when configPath is unset", () => {
  const result = resolveConfigPath(settings(), "/ws/folder");
  assert.deepEqual(result, { path: undefined });
});

test("buildInitializationOptions omits configPath entirely when unset - never sends an explicit null", () => {
  const result = buildInitializationOptions(settings(), "/ws/folder");
  assert.deepEqual(result, { options: {} });
  assert.equal("configPath" in result.options, false);
});

test("buildInitializationOptions sends the resolved configPath when a folder is open", () => {
  const result = buildInitializationOptions(settings({ configPath: "rel/path.toml" }), "/ws/folder");
  assert.deepEqual(result, { options: { configPath: "/ws/folder/rel/path.toml" } });
});

test("buildInitializationOptions sends the raw configPath and propagates the warning with no folder open", () => {
  const result = buildInitializationOptions(settings({ configPath: "rel/path.toml" }), undefined);
  assert.equal(result.options.configPath, "rel/path.toml");
  assert.equal(typeof result.warning, "string");
});
