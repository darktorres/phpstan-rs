import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import { resolveServerCommand, findPhpstanRsOnPath } from "./serverResolution";
import { resolveServerPath } from "./serverDownload";

export { resolveServerCommand, findPhpstanRsOnPath, resolveServerPath };

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const config = vscode.workspace.getConfiguration("phpstan-rs");
  const configuredPath = config.get<string>("serverPath");

  // `resolveServerCommand`/`findPhpstanRsOnPath` (4.1) win first when a
  // phpstan-rs is already reachable - an explicit setting or an existing
  // PATH install (e.g. `cargo install`, a distro package) - so a user's
  // own choice of binary is never silently shadowed by an auto-download.
  // Only once both come up empty does `resolveServerPath` (4.2) take
  // over: it re-checks `configuredPath` itself (a no-op here, since it
  // already failed above) and then falls through to its own cached-
  // binary/download pipeline.
  let command = resolveServerCommand(configuredPath, () => findPhpstanRsOnPath());
  if (!command) {
    try {
      command = await resolveServerPath(undefined, context.globalStorageUri.fsPath);
    } catch (err: unknown) {
      void vscode.window.showErrorMessage(
        `phpstan-rs: ${err instanceof Error ? err.message : String(err)}`,
      );
      return;
    }
  }

  // `["lsp"]` - the real subcommand `Command::Lsp => lsp::run()`
  // dispatches on (src/main.rs), not a `--lsp` flag (see the doc-comment
  // fix at src/lsp.rs:1 this item's own Rust-side change makes).
  const serverOptions: ServerOptions = {
    command,
    args: ["lsp"],
    transport: TransportKind.stdio,
  };
  // `outputChannelName`, not a manually-created `outputChannel` - since
  // vscode-languageclient 10, `LanguageClientOptions.outputChannel`
  // requires a `LogOutputChannel` specifically (`window.
  // createOutputChannel` alone returns the older, plain `OutputChannel`
  // type); letting the client create and own its own named channel is
  // the simpler, version-correct way to get one - this is what 4.3's
  // own `trace.server` setting will control.
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "php" }],
    outputChannelName: "phpstan-rs Language Server",
  };

  client = new LanguageClient(
    "phpstan-rs",
    "phpstan-rs Language Server",
    serverOptions,
    clientOptions,
  );

  // `client.start()` returns a `Promise` - a spawn failure (ENOENT/
  // EACCES) surfaces as a REJECTION, not a thrown exception, so this
  // needs `.catch()`, not a `try`/`catch` around the call itself (which
  // would never observe an async rejection).
  client.start().catch((err: unknown) => {
    void vscode.window.showErrorMessage(
      `phpstan-rs: failed to start the language server (${command}): ${String(err)}`,
    );
  });
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
