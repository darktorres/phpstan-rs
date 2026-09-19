// PURE data module - zero `vscode` import of any kind, not even
// type-only, so config.test.ts loads and runs in a plain node process
// with no mock and no extension host. Any future `import ... from
// 'vscode'` here is a regression to push into settings.ts/extension.ts
// instead.

import * as path from "path";

export interface Settings {
  enable: boolean;
  configPath: string | null;
  serverPath: string | null;
}

export interface ResolvedPath {
  path: string | undefined;
  warning?: string;
}

/**
 * Resolves a relative `configPath` against `folderPath` via
 * `path.resolve` (never string concatenation - the same separator-bug
 * class 3.7 addresses server-side). When `configPath` is relative and
 * no folder is open (single-file mode), returns the raw, unresolved
 * value plus a `.warning` string as DATA rather than calling any vscode
 * API itself - the caller (`extension.ts`) decides whether/how to show
 * it.
 */
export function resolveConfigPath(
  settings: Settings,
  folderPath: string | undefined,
): ResolvedPath {
  if (settings.configPath === null) {
    return { path: undefined };
  }
  if (path.isAbsolute(settings.configPath)) {
    return { path: settings.configPath };
  }
  if (folderPath === undefined) {
    return {
      path: settings.configPath,
      warning:
        `phpstan-rs.configPath ("${settings.configPath}") is relative, but no workspace ` +
        "folder is open to resolve it against - using it unresolved.",
    };
  }
  return { path: path.resolve(folderPath, settings.configPath) };
}

/**
 * Builds the `initializationOptions` object sent to the server. Omits
 * any key the user didn't actually set (never sends an explicit `null`)
 * so `InitializeParams.initialization_options`'s already-tolerant
 * `Option<Value>` decoding never has to special-case an absent vs. null
 * field once a real consumer (1.1) lands.
 */
export function buildInitializationOptions(
  settings: Settings,
  folderPath: string | undefined,
): { options: Record<string, unknown>; warning?: string } {
  const resolved = resolveConfigPath(settings, folderPath);
  const options: Record<string, unknown> = {};
  if (resolved.path !== undefined) {
    options.configPath = resolved.path;
  }
  return resolved.warning === undefined ? { options } : { options, warning: resolved.warning };
}
