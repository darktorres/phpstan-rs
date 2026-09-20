import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  State,
} from "vscode-languageclient/node";
import { getSettings } from "./settings";
import { buildInitializationOptions } from "./config";
import { resolveServerBinary } from "./serverBinary";

let client: LanguageClient | undefined;

function firstFolderPath(): string | undefined {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

/**
 * (Re)builds and starts a fresh `LanguageClient` from a current settings
 * snapshot, stopping any previously running client first. Used by both
 * `activate()` and `phpstan-rs.restartServer` - the restart command is
 * the practical way a user applies a changed `configPath`/`serverPath`/
 * `enable` before 3.3 gives the server itself a live-reload path;
 * without it, only VS Code's own "Developer: Reload Window" recovers.
 */
async function startClient(context: vscode.ExtensionContext): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }

  const settings = getSettings();
  if (!settings.enable) {
    return;
  }

  let command: string;
  try {
    command = await resolveServerBinary(settings, context.globalStorageUri.fsPath);
  } catch (err: unknown) {
    void vscode.window.showErrorMessage(
      `phpstan-rs: ${err instanceof Error ? err.message : String(err)}`,
    );
    return;
  }

  const { options: initializationOptions, warning } = buildInitializationOptions(
    settings,
    firstFolderPath(),
  );
  if (warning) {
    void vscode.window.showWarningMessage(`phpstan-rs: ${warning}`);
  }

  // `["lsp"]` - the real subcommand `Command::Lsp => lsp::run()`
  // dispatches on (src/main.rs), not a `--lsp` flag (see the doc-comment
  // fix at src/lsp.rs:1 item 4.2's Rust-side change made). `transport`
  // is deliberately omitted, not set to `TransportKind.stdio`: for an
  // `Executable` server, vscode-languageclient only spawns via plain
  // stdio pipes either way (`transport === undefined || transport ===
  // TransportKind.stdio` is one branch in its own `node/main.js`), but
  // an explicit `TransportKind.stdio` additionally appends a literal
  // `--stdio` CLI argument the spawned process is expected to parse -
  // `phpstan-rs lsp` takes no such flag (stdio is its only transport,
  // unconditional) and rejects it outright, crashing the server 5 times
  // in a row until vscode-languageclient gives up restarting it.
  const serverOptions: ServerOptions = {
    command,
    args: ["lsp"],
  };
  // `outputChannelName`, not a manually-created `outputChannel` - since
  // vscode-languageclient 10, `LanguageClientOptions.outputChannel`
  // requires a `LogOutputChannel` specifically (`window.
  // createOutputChannel` alone returns the older, plain `OutputChannel`
  // type); letting the client create and own its own named channel is
  // the simpler, version-correct way to get one. `trace.server` binds
  // itself automatically off the client id below ("phpstan-rs") -
  // no code needed for that setting at all.
  // `synchronize.configurationSection` makes the client send
  // `workspace/didChangeConfiguration` on any `phpstan-rs.*` change;
  // the server's `handle_notification` has a catch-all `_ => {}` arm,
  // so this is inert (not an error) until 3.3 gives it a real listener -
  // wiring the client half now costs nothing.
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "php" }],
    outputChannelName: "phpstan-rs Language Server",
    initializationOptions,
    synchronize: { configurationSection: "phpstan-rs" },
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

export interface ExtensionExports {
  /** Exposed so an integration test can distinguish "activate() has
   * returned" (VS Code's own `isActive`/`exports` signal, which fires
   * the instant `activate()` resolves) from "the LanguageClient has
   * actually finished spawning and handshaking" - two different, non-
   * simultaneous events, since `activate()` never awaits `client.start()`
   * itself. */
  getClientState: () => State | undefined;
}

export async function activate(context: vscode.ExtensionContext): Promise<ExtensionExports> {
  context.subscriptions.push(
    vscode.commands.registerCommand("phpstan-rs.restartServer", () => startClient(context)),
  );

  // Only these three settings change the spawned process or its args -
  // a plain `configurationSection` push can't apply them, since the
  // process is already running with different args than the new
  // settings call for. `trace.server` self-applies (no prompt needed);
  // a `configPath` file-content edit (not the *setting*) is 3.3's own
  // scope once it exists. A settings change landing between
  // `initialize` and this handler being wired up is not handled
  // specially - low risk given VS Code's own event-ordering guarantees.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      const affected =
        event.affectsConfiguration("phpstan-rs.enable") ||
        event.affectsConfiguration("phpstan-rs.serverPath") ||
        event.affectsConfiguration("phpstan-rs.configPath");
      if (!affected) {
        return;
      }
      void vscode.window
        .showInformationMessage("phpstan-rs settings changed", "Restart Server")
        .then((choice) => {
          if (choice === "Restart Server") {
            void startClient(context);
          }
        });
    }),
  );

  await startClient(context);

  return { getClientState: () => client?.state };
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
