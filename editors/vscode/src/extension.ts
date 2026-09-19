import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import { resolveServerCommand, findPhpstanRsOnPath } from "./serverResolution";

export { resolveServerCommand, findPhpstanRsOnPath };

let client: LanguageClient | undefined;
// Gates the "no server binary found" message to once per extension-host
// session, not once per opened .php file - `activate()` can in principle
// run again within the same session across certain reload scenarios, in
// which case this resets; accepted for this item's scope.
let missingServerWarningShown = false;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("phpstan-rs");
  const configuredPath = config.get<string>("serverPath");
  const command = resolveServerCommand(configuredPath, () =>
    findPhpstanRsOnPath(),
  );

  if (!command) {
    if (!missingServerWarningShown) {
      missingServerWarningShown = true;
      void vscode.window.showErrorMessage(
        "phpstan-rs: no server binary found. Set phpstan-rs.serverPath in your settings, " +
          "or make sure a phpstan-rs executable is on your PATH.",
      );
    }
    return;
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
