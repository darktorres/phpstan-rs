// Thin, vscode-dependent wrapper - deliberately NOT unit-tested in
// isolation (see serverDownload.test.ts's sibling, extension.test.ts,
// for why: this is a 3-line pass-through with no branching logic worth
// isolating, and testing it would require the vscode-module mock/
// @vscode/test-electron infrastructure this split exists to avoid).

import * as vscode from "vscode";
import { Settings } from "./config";

/** The only place in the extension that calls
 * `vscode.workspace.getConfiguration` for these settings - the defaults
 * here MUST literally match package.json's own `contributes.configuration`
 * declared defaults. */
export function getSettings(folder?: vscode.WorkspaceFolder): Settings {
  const config = vscode.workspace.getConfiguration("phpstan-rs", folder?.uri);
  return {
    enable: config.get<boolean>("enable", true),
    configPath: config.get<string | null>("configPath", null),
    serverPath: config.get<string | null>("serverPath", null),
  };
}
