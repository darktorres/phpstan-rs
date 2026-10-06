//! A minimal Language Server Protocol server, run via `phpstan-rs lsp`.
//! Communicates over stdio using LSP's standard JSON-RPC-over-`Content-
//! Length`-framed-messages transport, via the `lsp-server`/`lsp-types`
//! crates (protocol framing/message shapes and typed request/notification
//! params, respectively) rather than hand-rolling either against raw
//! `serde_json::Value` - this project already depends on `serde`/
//! `serde_json` for equally "boring, solved" infrastructure elsewhere
//! (composer.json parsing, this project's own config file), so hand-
//! rolling the LSP/JSON-RPC layer specifically stopped being the more
//! consistent choice once those were already dependencies.
//!
//! `diagnostics::Diagnostic` carries a line and column range, converted
//! here to LSP's 0-indexed line/character convention with the file's line
//! index, the character counted in the unit `initialize` agreed on (UTF-16
//! code units, or bytes when the client offers UTF-8). The `location`
//! symbolic label (e.g. `"ClassName::methodName"`) is still folded into
//! the message text rather than dropped - LSP has no field for it.

use rustc_hash::{FxHashMap, FxHashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use types::Name;

use crossbeam_channel::Receiver;
use lsp_server::{
    ErrorCode, Message, Notification as LspNotification, Request, RequestId, Response,
};
use lsp_types::{
    CancelParams, CodeActionKind, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CompletionOptions, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidOpenTextDocumentParams, DocumentHighlight,
    DocumentHighlightParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InlayHintParams, Location, MarkedString, MessageType,
    OneOf, ParameterInformation, ParameterLabel, Position as LspPosition, PositionEncodingKind,
    Range as LspRange, ServerCapabilities, ServerInfo, SignatureHelp, SignatureHelpOptions,
    SignatureHelpParams, SignatureInformation, SymbolInformation, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, WorkDoneProgress, WorkDoneProgressBegin, WorkDoneProgressEnd,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use crate::checker::Checker;
use crate::project::session::{ParseFailure, Session};
use crate::project::source::ContentHash;
use crate::project::use_sites;
use crate::project::{Project, ProjectSettings, Records};
use crate::registry::{FunctionReflection, format_signature, param_label};
use syntax::ast::ClassKind;
use syntax::span::{LineIndex, Span, Unit};
use types::Type;

mod completion;
mod debounce;
mod document_symbol;
mod inlay_hints;
mod navigation;
mod protocol;
mod semantic_tokens;
pub mod session_record;
mod settings;
mod signature_help;
mod workspace_config;
mod workspace_symbol;

#[cfg(test)]
use protocol::percent_decode;
use protocol::{
    Positions, diagnostic_fix_to_code_action, log_message_notification, offset_from_lsp,
    path_to_uri, progress_notification, publish_diagnostics_notification, pump_messages,
    range_to_lsp, ranges_overlap, show_message_notification, to_request_id, uri_to_path,
    write_message,
};

/// `owner`'s member `member` as the use-site index keys it: a method by
/// its declared name, since PHP compares method names without case, unless
/// a property or constant has this very name.
fn member_target(
    registry: &crate::registry::Registry,
    owner: String,
    member: &str,
) -> use_sites::RefTarget {
    let names_another_member = registry.classes().get(&owner).is_some_and(|info| {
        info.properties().contains_key(member) || info.consts().contains_key(member)
    });
    let member = if names_another_member {
        member.to_string()
    } else {
        registry.declared_method_name(&owner, member)
    };
    use_sites::RefTarget::Member(owner, member)
}

/// Resolves a receiver's own (possibly multi-class) type down to every
/// class that actually declares `member` - a `Union`/`Intersection`
/// receiver can have more than one real answer (`(Foo&Bar) $x` where
/// both declare `greet()`), each a genuinely separate, correct
/// navigation target, not an arbitrary "first one wins" guess. Deduped
/// by the member's declaring owner (not the candidate class itself), via
/// `lookup_*_with_owner`'s own hierarchy walk - two union members that
/// both inherit the same declaration collapse to one entry.
fn resolve_member_owners(
    registry: &crate::registry::Registry,
    ty: &Type,
    member: &str,
) -> Vec<String> {
    let candidates: Vec<String> = ty
        .object_class_names()
        .iter()
        .map(|c| c.to_string())
        .collect();
    let mut owners: Vec<String> = Vec::new();
    for class in candidates {
        let owner = registry
            .lookup_method_with_owner(&class, member)
            .map(|(o, _)| o)
            .or_else(|| {
                registry
                    .lookup_property_with_owner(&class, member)
                    .map(|(o, _)| o)
            })
            .or_else(|| {
                registry
                    .lookup_const_with_owner(&class, member)
                    .map(|(o, _)| o)
            });
        if let Some(o) = owner.map(|o| o.to_string())
            && !owners.contains(&o)
        {
            owners.push(o);
        }
    }
    owners
}

/// The member name a `Member`/`Instance` `Resolved` carries, or `None`
/// for every other variant - `Workspace::same_symbol`'s own quick pre-
/// filter before the more expensive owner-normalization/probe work in
/// `member_owners_of`.
fn member_name_hint(r: &navigation::Resolved) -> Option<&str> {
    match r {
        navigation::Resolved::Member(_, name) => Some(name.as_str()),
        navigation::Resolved::Instance { member, .. } => Some(member.as_str()),
        _ => None,
    }
}

/// Captured at `"initialize"`, consumed exactly once at `"initialized"` -
/// the scan (and its optional `$/progress` announcement) must never run
/// before the client has acknowledged the `InitializeResult`, so
/// `initialize`'s own response is never gated on how long it takes.
struct PendingWorkspaceScan {
    roots: Vec<PathBuf>,
    progress_token: Option<lsp_types::ProgressToken>,
}

/// One workspace folder's analysis: the project and session its
/// configuration resolved to, what its checks found, and the documents
/// the client has open in it. A document belongs to the deepest folder
/// that contains it (`Server::workspace_of`); a document outside every
/// folder belongs to the workspace without a root.
struct Workspace {
    // The folder this workspace analyses, `None` for the workspace of the
    // documents no folder contains.
    root: Option<PathBuf>,
    // The folders nested inside this one, whose documents belong to them:
    // nothing found in a path under one is published from here. For the
    // workspace without a root, every folder.
    nested_roots: Vec<PathBuf>,
    project: Project,
    // The session `root`'s configuration resolved to, which lists the
    // files `root` loads.
    session: Option<Session>,
    // The last `ResolvedWorkspaceConfig` actually applied to `project` -
    // `reload_configuration`'s no-op fast path compares a freshly resolved
    // value against this via real `PartialEq` (never a `Debug`-formatted
    // fingerprint - `SqlReflectionConfig`'s own `Debug` impl always prints
    // its `password` field as a constant redacted token, which would make
    // a password-only edit invisible to a string-based comparison).
    last_applied_config: Option<workspace_config::ResolvedWorkspaceConfig>,
    // Every URI the client currently has open or has otherwise seen, so
    // the workspace scan and `didOpen`/`didChange` share one
    // path->diagnostics story instead of the workspace scan being
    // invisible to later incremental updates. Also carries the last
    // known LSP document version for that path (`None` if only ever seen
    // via the workspace scan or a `workspace/didChangeWatchedFiles`
    // event - neither carries a document version, only `didOpen`/
    // `didChange` do), so `publishDiagnostics` can be stamped with it -
    // see `uri_and_version_for`.
    open_uris: FxHashMap<PathBuf, (Uri, Option<i32>)>,
    // What the checks last found in every path this workspace has ever
    // checked, before `apply_post_filters` - every level's findings, so a
    // configuration change that only changes which are shown publishes
    // them again without checking anything (`reload_configuration`).
    // `code_actions` reads this instead of re-running a whole-file
    // checker pass per request (a real editor's lightbulb fires
    // `textDocument/codeAction` far more often than actual edits, e.g. on
    // cursor movement). Kept in sync by every diagnostic-producing path
    // (`check_workspace`, `publish_diagnostics_for`), cleared for a path
    // a removed folder or `handle_watched_files_changed`'s Deleted branch
    // drops. Safe under the one-message-at-a-time `run_loop`: a
    // `codeAction` request can only ever see whatever the most recently
    // processed `didChange`/scan already committed, never a torn or
    // stale-relative-to-an-earlier-unprocessed-edit state.
    last_diagnostics: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    // What the whole-program checks last found, by path, shown beside
    // `last_diagnostics`: a file's edit does not change them, a save does
    // (`recheck_whole_program`).
    whole_program: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    // The analysed files that do not parse as last loaded or edited, each
    // reported where it fails; a file leaves when it parses again or is
    // removed.
    parse_failures: FxHashMap<PathBuf, ParseFailure>,
    // The exact current text `didOpen`/`didChange` last sent for a path,
    // regardless of whether it parses - the project's source and syntax
    // tree only ever reflect the last *successfully parsed* version, but
    // `textDocument/completion`'s member-completion path needs the LIVE
    // (very often mid-edit and unparseable - `$x->` alone is a syntax
    // error) buffer text to tokenize and to extract a receiver
    // expression's own text from. See `completion`'s own module doc
    // comment for the full mechanism this feeds.
    latest_text: FxHashMap<PathBuf, String>,
    // The documents the client has open, which the session reads in place
    // of the disk's files (`Session::set_overlay`).
    open_documents: FxHashSet<PathBuf>,
    // Bumped by `handle_open_or_change` on every `didOpen`/`didChange` for
    // a path - `inlay_hints`'s cache-invalidation key. A path never opened/
    // changed this session (only ever seen via the workspace scan) has an
    // implicit version of 0, consistent with its first request computing
    // and caching at that same version.
    file_versions: FxHashMap<PathBuf, u64>,
    // `textDocument/inlayHint`'s version-keyed whole-file hint cache - see
    // `inlay_hints`'s own doc comment for why a cache is needed at all
    // (VS Code re-pulls inlay hints on edits, not just scroll/viewport
    // changes, so serving a request against an unchanged document from
    // cache is what actually saves a repeated `check_program` pass).
    #[allow(clippy::type_complexity)]
    inlay_hint_cache: FxHashMap<PathBuf, (u64, Vec<(Span, Name)>, Vec<(Span, Type)>)>,
    // What a column of a position counts: UTF-16 code units unless the
    // client offered UTF-8 and `initialize` agreed to it.
    unit: Unit,
}

struct Server {
    // The workspace of each folder the client has open, in the order
    // they were added.
    folders: Vec<Workspace>,
    // The workspace of the documents no folder contains.
    unrooted: Workspace,
    // The process's worker pool (`project::worker_pool`), which every
    // workspace check runs on.
    pool: Arc<rayon::ThreadPool>,
    // What a column of a position counts: UTF-16 code units unless the
    // client offered UTF-8 and `initialize` agreed to it.
    unit: Unit,
    // Set once a real `shutdown` request has been handled - `run_loop`'s
    // own `exit` arm consults this to pick the LSP spec's mandated
    // process exit code (0 if `shutdown` preceded `exit`, 1 otherwise).
    shutdown_received: bool,
    // Ids/methods of requests this server has sent to the client and is
    // still waiting on a reply for - `Incoming`'s `I` is unused (`()`,
    // nothing here tracks inbound requests through this field; see
    // `pending_incoming_requests` below for that), `Outgoing`'s `O`
    // stores just the method name for diagnostics.
    pending_requests: lsp_server::ReqQueue<(), &'static str>,
    // Client-assigned ids of requests THIS server is still answering,
    // each paired with a cancellation flag - separate from
    // `pending_requests` above (that one tracks the opposite direction,
    // server-initiated requests) since `ReqQueue<I, O>` fixes one `I`
    // type for both halves and `pending_requests` already commits that
    // slot to `()`. Populated by `handle_request` at the top of every
    // request and drained by either a normal response
    // (`respond_unless_canceled`) or a `"$/cancelRequest"` notification -
    // whichever reaches a given id first. No handler reads the flag yet
    // (every request today is synchronous and sub-millisecond); it
    // exists so a future long-running request handler could poll it,
    // contingent on one actually being added - the roadmap's currently
    // identified slow operations (the workspace scan, debounced
    // rechecks) are notifications/background work reported via
    // `$/progress`, not a cancellable request, so that consumer is not
    // guaranteed to ever materialize.
    pending_incoming_requests: lsp_server::Incoming<Arc<AtomicBool>>,
    // Set by `"initialize"`, taken and acted on by `"initialized"` - see
    // `PendingWorkspaceScan`'s own doc comment for why the scan itself
    // can't just run inline in `"initialize"`.
    pending_workspace_scan: Option<PendingWorkspaceScan>,
    // The folders whose workspaces are being built on the pool, with what
    // the client sent for them meanwhile.
    loading: Vec<LoadingFolder>,
    // The workspaces the pool has built, which the message loop installs.
    loaded_tx: crossbeam_channel::Sender<LoadedFolder>,
    loaded_rx: crossbeam_channel::Receiver<LoadedFolder>,
    // The scan `"initialized"` started: the work-done token its progress
    // reports on, and the roots still loading.
    scan_progress: Option<(lsp_types::ProgressToken, FxHashSet<PathBuf>)>,
    // A `phpstan-rs.configPath` setting override, as the client spelled
    // it: applied on top of each folder's own default `phpstan-rs.toml`
    // lookup by every subsequent `configure`/`reload_configuration` call,
    // a relative path against that folder. `None` until a
    // `workspace/didChangeConfiguration` notification sets one.
    config_path_override: Option<PathBuf>,
    // `phpstan-rs.enable` - "stop showing squiggles", not "stop the
    // language server": hover/definition/documentSymbol keep working
    // while `false`, only `publishDiagnostics` goes empty.
    enabled: bool,
    // Captured once at `"initialize"` from
    // `capabilities.workspace.configuration` - whether this client
    // supports a server-initiated `workspace/configuration` pull. A real
    // `vscode-languageclient` client does and needs it: it always sends
    // `workspace/didChangeConfiguration` with an empty/placeholder
    // `settings` value, so the pushed-settings path alone would never see
    // an actual `phpstan-rs.*` value from it.
    supports_configuration_pull: bool,
    // Captured once at `"initialize"` from
    // `capabilities.workspace.didChangeWatchedFiles.dynamicRegistration` -
    // whether this client can even be asked to watch files for this
    // server. LSP has no static `ServerCapabilities` field for file
    // watching, only this dynamic-registration request/response
    // mechanism, so unlike `hover_provider`/etc. this can't be
    // advertised in `"initialize"`'s own response.
    supports_watched_files: bool,
    // The version `initialize` reports, the binary's own.
    version: String,
    // Every section a workspace's `phpstan-rs.toml` may set.
    config_schema: Arc<config::Schema>,
}

/// A folder whose workspace is being built on the pool. What the client
/// sends for a document under it meanwhile waits here, to be applied to
/// the workspace as it is installed.
struct LoadingFolder {
    root: PathBuf,
    // The documents the client opened or edited, as they last were.
    documents: FxHashMap<PathBuf, OpenDocument>,
    // The watched-file events for paths under the folder, in order.
    watched: Vec<lsp_types::FileEvent>,
}

impl LoadingFolder {
    /// Notes the document at `path` as the client last sent it.
    fn record_document(&mut self, path: &Path, uri: &Uri, text: &str, version: Option<i32>) {
        self.documents.insert(
            path.to_path_buf(),
            OpenDocument {
                uri: uri.clone(),
                version,
                text: text.to_string(),
            },
        );
    }
}

/// What a pool task built for the folder at `root`: its workspace and the
/// messages the load gave, or what made the task fail.
struct LoadedFolder {
    root: PathBuf,
    outcome: Result<(Workspace, Vec<Message>), String>,
}

/// Who a path belongs to, the deepest of the folders and of those loading.
enum Owner {
    Folder(usize),
    Loading(usize),
    Unrooted,
}

/// The message that ends the progress report of the workspace scan.
fn scan_progress_end(token: lsp_types::ProgressToken) -> Message {
    progress_notification(
        token,
        WorkDoneProgress::End(WorkDoneProgressEnd { message: None }),
    )
}

/// A document the client has open: its URI and version, and its text.
struct OpenDocument {
    uri: Uri,
    version: Option<i32>,
    text: String,
}

/// What the client's settings and the process give each folder's
/// configuration.
struct ConfigInputs<'a> {
    // The `phpstan-rs.configPath` setting, a relative path against the
    // folder.
    path_override: Option<&'a Path>,
    schema: &'a config::Schema,
    pool: &'a Arc<rayon::ThreadPool>,
}

impl ConfigInputs<'_> {
    /// Resolves the configuration of the folder at `root`.
    fn open(&self, root: &Path) -> workspace_config::OpenedWorkspace {
        let config_path = self.path_override.map(|path| root.join(path));
        workspace_config::open(root, config_path.as_deref(), self.schema, self.pool.clone())
    }
}

impl Workspace {
    fn new(root: Option<PathBuf>, unit: Unit) -> Self {
        let mut project = Project::new();
        // hover/definition's own text-split technique needs each file's
        // exact source text - see `Project::retain_source`'s own doc
        // comment for why this is opt-in and off elsewhere.
        project.apply_settings(ProjectSettings {
            retain_source: true,
            keeps_as_built: true,
            ..ProjectSettings::default()
        });
        Workspace {
            root,
            nested_roots: Vec::new(),
            project,
            session: None,
            last_applied_config: None,
            open_uris: FxHashMap::default(),
            last_diagnostics: FxHashMap::default(),
            whole_program: FxHashMap::default(),
            parse_failures: FxHashMap::default(),
            latest_text: FxHashMap::default(),
            open_documents: FxHashSet::default(),
            file_versions: FxHashMap::default(),
            inlay_hint_cache: FxHashMap::default(),
            unit,
        }
    }

    /// Builds the workspace of the folder at `root`: its configuration, its
    /// files loaded, the registry's derived state computed.
    fn load(root: PathBuf, unit: Unit, inputs: &ConfigInputs) -> (Workspace, Vec<Message>) {
        let mut workspace = Workspace::new(Some(root), unit);
        let mut messages = workspace.configure(inputs);
        messages.extend(workspace.load_workspace_files());
        workspace.project.finish_loading();
        (workspace, messages)
    }

    /// Whether a document at `path` belongs to this workspace rather than
    /// to a folder nested inside it.
    fn owns(&self, path: &Path) -> bool {
        !self.nested_roots.iter().any(|root| path.starts_with(root))
    }

    /// Resolves the folder's own `phpstan-rs.toml` as `check-project`
    /// resolves it and applies it to `self.project`, so the folder's
    /// config produces the same `Project` state (and the same
    /// published-diagnostic set) `check-project` would build/report for
    /// the identical tree. The session it resolved lists the files
    /// `load_workspace_files` loads. Tells the user what the
    /// configuration holds that is wrong.
    fn configure(&mut self, inputs: &ConfigInputs) -> Vec<Message> {
        let Some(root) = self.root.clone() else {
            return Vec::new();
        };
        let workspace = inputs.open(&root);
        let messages = self.configuration_problems(&workspace.config);
        self.adopt_session(workspace.session);
        self.apply_resolved_config(workspace.config);
        messages
    }

    /// The message that tells the user what in `resolved` was wrong and
    /// taken as unset, which `last_applied_config` has not been told of.
    fn configuration_problems(
        &self,
        resolved: &workspace_config::ResolvedWorkspaceConfig,
    ) -> Vec<Message> {
        let told = self
            .last_applied_config
            .as_ref()
            .map_or(&[][..], |applied| applied.problems.as_slice());
        resolved
            .problems
            .iter()
            .filter(|problem| !told.contains(problem))
            .map(|problem| {
                show_message_notification(MessageType::WARNING, format!("phpstan-rs: {problem}"))
            })
            .collect()
    }

    /// The apply half of `configure`, factored out so `reload_configuration`
    /// can apply an already-resolved value (its own no-op fast path needs
    /// the resolved value in hand *before* deciding whether to apply it,
    /// which `configure`'s resolve-then-apply-in-one-call shape doesn't
    /// allow).
    fn apply_resolved_config(&mut self, resolved: workspace_config::ResolvedWorkspaceConfig) {
        self.project.apply_settings(resolved.settings.clone());
        self.apply_report_config(resolved);
    }

    /// The part of `apply_resolved_config` that decides which findings
    /// are published, not what the checks find, keeping `resolved` as
    /// `self.last_applied_config`.
    fn apply_report_config(&mut self, resolved: workspace_config::ResolvedWorkspaceConfig) {
        self.last_applied_config = Some(resolved);
    }

    /// Finalizes `findings` as `check-project` finalizes its own (`Finalize`),
    /// scoped to whatever partial result set `update_file`'s own
    /// transitive-dependents pass just produced. Never surfaces the
    /// baseline's stale-entry report here: its staleness detection assumes
    /// it's run against the whole project's diagnostics at once, and
    /// treating every other baselined file as "no longer matches" on every
    /// keystroke would be a constant false-positive warning storm.
    fn apply_post_filters(
        &self,
        mut results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    ) -> FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>> {
        if let Some(applied) = &self.last_applied_config {
            applied.finalize.apply(&self.project, &mut results);
        }
        results
    }

    /// What is held of each of `paths` that has anything: the findings of
    /// its checks, then the whole-program checks'.
    fn held_findings<'p>(
        &self,
        paths: impl IntoIterator<Item = &'p PathBuf>,
    ) -> FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>> {
        paths
            .into_iter()
            .filter_map(|path| {
                let own = self.last_diagnostics.get(path);
                let whole_program = self.whole_program.get(path);
                if own.is_none() && whole_program.is_none() {
                    return None;
                }
                let found = own
                    .into_iter()
                    .chain(whole_program)
                    .flatten()
                    .cloned()
                    .collect();
                Some((path.clone(), found))
            })
            .collect()
    }

    /// Takes `session` as the workspace's, reading the open documents in
    /// place of the disk's files, and gives back the one it replaces.
    fn adopt_session(&mut self, mut session: Session) -> Option<Session> {
        for path in &self.open_documents {
            if let Some(text) = self.latest_text.get(path) {
                session.set_overlay(path, text.as_bytes().to_vec());
            }
        }
        self.session.replace(session)
    }

    /// Notes that `path` is open with `text`, which the workspace session
    /// reads from now on.
    fn open_document(&mut self, path: &Path, text: &str) {
        self.open_documents.insert(path.to_path_buf());
        self.project.open_file(path, text);
        if let Some(session) = &mut self.session {
            session.set_overlay(path, text.as_bytes().to_vec());
        }
    }

    /// Notes that `path` is no longer open: the disk's file is read again.
    fn close_document(&mut self, path: &Path) {
        self.open_documents.remove(path);
        self.project.close_file(path);
        if let Some(session) = &mut self.session {
            session.clear_overlay(path);
        }
    }

    /// Takes the open document `path` out of this workspace, for another
    /// to open.
    fn take_document(&mut self, path: &Path) -> Option<OpenDocument> {
        if !self.open_documents.contains(path) {
            return None;
        }
        let (uri, version) = self.open_uris.remove(path)?;
        let text = self.latest_text.remove(path)?;
        self.file_versions.remove(path);
        self.close_document(path);
        Some(OpenDocument { uri, version, text })
    }

    /// The bytes of the file at `path`: an open document's text, or the
    /// disk's.
    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        match &self.session {
            Some(session) => session.read(path),
            None => std::fs::read(path),
        }
    }

    /// Reads and parses `files` in parallel on the worker pool, inserting
    /// each as context (vendor/scan/stub, never analyzed or reported) or
    /// analyzed (a real, first-party file under `root`) in order. A parse
    /// failure is logged (both to stderr and as a `window/logMessage`
    /// notification) and skipped, not fatal to the rest of the scan.
    fn load_files(&mut self, files: &[(PathBuf, bool)], messages: &mut Vec<Message>) {
        let Some(session) = &self.session else {
            return;
        };
        let role_of = |analysed: bool| -> Vec<PathBuf> {
            files
                .iter()
                .filter(|(_, is_analysed)| *is_analysed == analysed)
                .map(|(path, _)| path.clone())
                .collect()
        };
        let added = session.load_into(&mut self.project, &role_of(false), &role_of(true));
        for (path, failure) in &added.parse_failures {
            self.parse_failures.insert(path.clone(), failure.clone());
        }
        for (path, failure) in added
            .context_failures
            .into_iter()
            .chain(added.parse_failures)
        {
            eprintln!(
                "phpstan-rs lsp: skipping {} - failed to parse: {}",
                path.display(),
                failure.message
            );
            messages.push(log_message_notification(
                MessageType::WARNING,
                format!(
                    "phpstan-rs: skipping {} - failed to parse: {}",
                    path.display(),
                    failure.message
                ),
            ));
        }
    }

    /// Loads the files the workspace session lists that are not loaded yet:
    /// the context files first, then the analysed ones, each in the
    /// session's order (a file an earlier folder, or a `didOpen`, loaded is
    /// left as it is), so the cross-file registry and dependency graph are
    /// populated from the very first diagnostic, not built up one
    /// `didOpen` at a time. Deliberately does NOT call
    /// `Project::finish_loading` itself - `Workspace::load` calls it once
    /// for the whole load; a bare call to this method with no
    /// `finish_loading` afterward would leave the registry's derived state
    /// (trait flattening, ancestor names, ...) stale for whatever it just
    /// loaded.
    ///
    /// **Context-before-analysed load order is a correctness invariant**:
    /// `Registry::insert_built_decl`'s handling of a second unconditional
    /// same-name declaration is itself order-sensitive - context-then-
    /// analyzed hits the `analyzed: true` branch (an outright overwrite,
    /// discarding the context entry's members), while analyzed-then-context
    /// would instead hit the `analyzed: false` branch (`existing.merge(...)`,
    /// folding the vendor info into the first-party entry) - a genuinely
    /// different `Project` state for the identical tree.
    fn load_workspace_files(&mut self) -> Vec<Message> {
        let mut messages = Vec::new();
        let Some(session) = &self.session else {
            return messages;
        };
        let (analysed, context) = session.listing();
        let to_load: Vec<(PathBuf, bool)> = context
            .into_iter()
            .map(|path| (path, false))
            .chain(analysed.into_iter().map(|path| (path, true)))
            .filter(|(path, _)| !self.project.is_loaded(path))
            .collect();
        self.load_files(&to_load, &mut messages);
        messages
    }

    /// Checks every analysed (first-party) file loaded and runs the
    /// whole-program checks, as `Session::analyse` does, keeping what they
    /// find in `last_diagnostics` and `whole_program` and returning the
    /// paths with findings now or before, in order: the workspace session
    /// runs the checks on the worker pool, one file's panic not taking
    /// down the push. Shared by `publish_workspace_diagnostics` (which
    /// then drops empty results) and `republish_known_files` (which must
    /// not, so a file previously flagged and now clean gets an explicit
    /// clearing notification).
    fn check_workspace(&mut self) -> Vec<PathBuf> {
        let Some(session) = &self.session else {
            return Vec::new();
        };
        let mut files = self.project.analyzed_paths();
        if files.is_empty() && self.parse_failures.is_empty() {
            return Vec::new();
        }
        // Deterministic order (matches load_workspace's own `files.sort()`
        // rationale) - HashMap/HashSet iteration order would otherwise
        // make repeated runs against an unchanged workspace look like a
        // spurious diff to anyone comparing published-message order.
        files.sort();
        let failures = self
            .parse_failures
            .iter()
            .map(|(path, failure)| (path.clone(), failure.clone()));
        let checked = session.check_loaded(&mut self.project, &files, failures, Records::default());
        for (path, msg) in &checked.check_panics {
            eprintln!(
                "phpstan-rs lsp: checking {} panicked: {msg}",
                path.display()
            );
        }
        let mut paths: Vec<PathBuf> = checked
            .results
            .keys()
            .chain(checked.whole_program.keys())
            .chain(self.whole_program.keys())
            .cloned()
            .collect();
        paths.sort();
        paths.dedup();
        self.last_diagnostics.extend(checked.results);
        self.whole_program = checked.whole_program;
        paths
    }

    /// Runs the whole-program checks again, which an edit does not: what
    /// they found is published for every file with findings now or before,
    /// a file now clean cleared.
    fn recheck_whole_program(&mut self, enabled: bool) -> Vec<Message> {
        if !enabled {
            return Vec::new();
        }
        let Some(session) = &self.session else {
            return Vec::new();
        };
        let found = session.whole_program(&mut self.project);
        let mut paths: Vec<PathBuf> = found
            .keys()
            .chain(self.whole_program.keys())
            .cloned()
            .collect();
        paths.sort();
        paths.dedup();
        self.whole_program = found;
        self.publish_shown(&paths, true)
    }

    /// Drops everything held of the findings in `path`.
    fn forget_findings(&mut self, path: &Path) {
        self.last_diagnostics.remove(path);
        self.whole_program.remove(path);
        self.parse_failures.remove(path);
    }

    /// Reports `error`, which the parse of `source` gave for `path`, as the
    /// loaders do: logged, and where `path` is an analysed file, published
    /// as its one finding until it parses again.
    fn parse_failed(
        &mut self,
        path: &Path,
        error: syntax::parser::ParseError,
        source: &[u8],
    ) -> Vec<Message> {
        eprintln!(
            "phpstan-rs lsp: skipping {} - failed to parse: {error}",
            path.display()
        );
        let mut messages = vec![log_message_notification(
            MessageType::WARNING,
            format!(
                "phpstan-rs: skipping {} - failed to parse: {error}",
                path.display()
            ),
        )];
        if self.project.is_analyzed(path) || self.parse_failures.contains_key(path) {
            let failure = ParseFailure::of(error, source);
            self.last_diagnostics
                .insert(path.to_path_buf(), vec![failure.diagnostic()]);
            self.parse_failures.insert(path.to_path_buf(), failure);
            messages.extend(self.publish_shown(&[path.to_path_buf()], true));
        }
        messages
    }

    /// Looks up the last known `(Uri, document version)` for `path` -
    /// falling back to a synthesized `Uri` and `None` when `path` was
    /// never seen via `didOpen`/`didChange` (e.g. a workspace-scanned-
    /// but-never-opened dependent, or one only ever touched via
    /// `workspace/didChangeWatchedFiles`, which carries no document
    /// version at all).
    fn uri_and_version_for(&self, path: &Path) -> (Uri, Option<i32>) {
        self.open_uris
            .get(path)
            .cloned()
            .unwrap_or_else(|| (path_to_uri(path), None))
    }

    /// The `publishDiagnostics` notification of `diags`, found in `path`,
    /// with the columns of the file's lines in the negotiated unit.
    fn diagnostics_message(
        &self,
        path: &Path,
        uri: Uri,
        version: Option<i32>,
        diags: &[diagnostics::Diagnostic],
    ) -> Message {
        let index = self.project.line_index(path).map(|index| &**index);
        publish_diagnostics_notification(uri, version, diags, index, self.unit)
    }

    /// The lines of the loaded file `path` with the unit of this session's
    /// columns.
    fn positions(&self, path: &Path) -> Option<Positions<'_>> {
        Some(Positions {
            index: self.project.line_index(path)?,
            unit: self.unit,
        })
    }

    /// Publishes a `textDocument/publishDiagnostics` notification for
    /// every analyzed file with findings - without this, a freshly opened
    /// workspace shows an empty Problems panel until each affected file
    /// happens to be individually opened or edited, which for most files
    /// in a real project never happens. Empty results are dropped: at an
    /// initial load nothing has been published for these paths yet, so
    /// there is nothing to clear.
    fn publish_workspace_diagnostics(&mut self) -> Vec<Message> {
        let checked = self.check_workspace();
        self.publish_shown(&checked, false)
    }

    /// `reload_configuration`'s own republish step, once it decides
    /// something actually changed. Unlike `publish_workspace_diagnostics`,
    /// this never drops an empty result: a file that had diagnostics
    /// under the old config and none under the new one needs an explicit
    /// empty `publishDiagnostics` to actually clear them client-side, not
    /// silence. While `!self.enabled`, skips rechecking entirely and just
    /// clears every currently-open file - phpstan-rs.enable means "stop
    /// showing squiggles", not "stop the language server".
    fn republish_known_files(&mut self, enabled: bool) -> Vec<Message> {
        if !enabled {
            return self.clear_open_files();
        }
        let checked = self.check_workspace();
        self.publish_shown(&checked, true)
    }

    /// `republish_known_files` for a configuration that changes only
    /// which findings are shown: the last findings of every checked path
    /// published again through the new post-filters, without checking
    /// anything.
    fn republish_last_diagnostics(&mut self, enabled: bool) -> Vec<Message> {
        if !enabled {
            return self.clear_open_files();
        }
        let mut paths: Vec<PathBuf> = self.last_diagnostics.keys().cloned().collect();
        paths.sort();
        self.publish_shown(&paths, true)
    }

    /// An empty `publishDiagnostics` for every file of this workspace that
    /// has had findings or a document open, which are dropped with it.
    fn clear_published(&self) -> Vec<Message> {
        let mut paths: Vec<&PathBuf> = self
            .last_diagnostics
            .keys()
            .chain(self.whole_program.keys())
            .chain(self.parse_failures.keys())
            .chain(self.open_uris.keys())
            .filter(|path| self.owns(path))
            .collect();
        paths.sort();
        paths.dedup();
        paths
            .into_iter()
            .map(|path| {
                let (uri, version) = self.uri_and_version_for(path);
                self.diagnostics_message(path, uri, version, &[])
            })
            .collect()
    }

    /// An empty `publishDiagnostics` for every open file.
    fn clear_open_files(&self) -> Vec<Message> {
        let unit = self.unit;
        self.open_uris
            .values()
            .cloned()
            .map(|(uri, version)| publish_diagnostics_notification(uri, version, &[], None, unit))
            .collect()
    }

    /// The `publishDiagnostics` notifications of what `last_diagnostics`
    /// holds for `paths`, as the post-filters show it; with `with_empty`,
    /// a path showing nothing is published too, clearing what it showed
    /// before.
    fn publish_shown(&self, paths: &[PathBuf], with_empty: bool) -> Vec<Message> {
        let mut shown = self.apply_post_filters(self.held_findings(paths));
        paths
            .iter()
            .filter(|path| self.owns(path))
            .filter_map(|path| {
                let diags = shown.remove(path).unwrap_or_default();
                if diags.is_empty() && !with_empty {
                    return None;
                }
                let (uri, version) = self.uri_and_version_for(path);
                Some(self.diagnostics_message(path, uri, version, &diags))
            })
            .collect()
    }

    fn handle_open_or_change(
        &mut self,
        uri: &Uri,
        text: &str,
        version: Option<i32>,
    ) -> Vec<Message> {
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        *self.file_versions.entry(path.clone()).or_insert(0) += 1;
        self.open_uris.insert(path.clone(), (uri.clone(), version));
        self.latest_text.insert(path.clone(), text.to_string());
        self.open_document(&path, text);
        // Opening a file exactly as it was loaded changes nothing in the
        // project, so only its own diagnostics need publishing.
        if self.project.content_hash(&path) == Some(ContentHash::of(text.as_bytes())) {
            if !self.last_diagnostics.contains_key(&path) {
                let found = self.project.check_file(&path);
                self.last_diagnostics.insert(path.clone(), found);
            }
            return self.publish_shown(&[path], true);
        }
        // Same graceful-degradation as load_workspace, and more exposed
        // in practice here: momentarily-invalid syntax is the normal
        // state while a file is being typed, not an edge case.
        // `update_file`'s `Result` makes a parse failure here a plain,
        // recoverable `Err` rather than unwinding and taking down the
        // whole LSP server process.
        let results = match self.project.update_file(&path, text) {
            Ok(results) => results,
            Err(e) => return self.parse_failed(&path, e, text.as_bytes()),
        };
        self.parse_failures.remove(&path);
        self.publish_diagnostics_for(results)
    }

    /// The debounce worker's own counterpart to `handle_open_or_change`
    /// (VSCode plan item 2.2) - identical except it drives `Project::
    /// update_file_cancellable` with a real `should_abort` closure
    /// instead of `update_file`, and takes an already-resolved `path`
    /// (the worker already has it as the `DebounceQueue` entry's own
    /// key) rather than re-deriving it from `uri`. `version` is a plain
    /// `i32`, not `Option<i32>` - a debounced edit always came from a
    /// real `didChange`, which always carries one.
    fn handle_debounced_change(
        &mut self,
        path: &Path,
        uri: &Uri,
        text: &str,
        version: i32,
        should_abort: impl Fn() -> bool + Sync,
    ) -> Vec<Message> {
        *self.file_versions.entry(path.to_path_buf()).or_insert(0) += 1;
        self.open_uris
            .insert(path.to_path_buf(), (uri.clone(), Some(version)));
        self.latest_text
            .insert(path.to_path_buf(), text.to_string());
        self.open_document(path, text);
        let results = match self
            .project
            .update_file_cancellable(path, text, should_abort)
        {
            Ok(results) => results,
            Err(e) => return self.parse_failed(path, e, text.as_bytes()),
        };
        self.parse_failures.remove(path);
        for stale in self.project.take_unchecked() {
            self.last_diagnostics.remove(&stale);
        }
        self.publish_diagnostics_for(results)
    }

    /// Routes a `workspace/didChangeWatchedFiles` batch through the same
    /// `update_file`/`remove_file` machinery `didOpen`/`didChange` and
    /// workspace-folder removal already use - an externally created,
    /// changed, or deleted file needs exactly the same recheck-and-
    /// republish treatment. Coalesces to the last event per path first: a
    /// client-side batch can legally carry e.g. Changed-then-Deleted for
    /// one path in a single notification, and only the final state
    /// matters (this is intra-batch coalescing only, not a cross-
    /// notification debounce - see this item's own doc comment).
    ///
    /// A file the project does not load, or a directory, created or
    /// deleted rechecks only the files whose includes looked for a file
    /// there (`Project::recheck_including`), as does a new PHP file that
    /// does not parse: either way, a file appeared or vanished.
    fn handle_watched_files_changed(&mut self, changes: Vec<lsp_types::FileEvent>) -> Vec<Message> {
        let mut last_by_path: FxHashMap<PathBuf, (Uri, lsp_types::FileChangeType)> =
            FxHashMap::default();
        for change in changes {
            let Some(path) = uri_to_path(&change.uri) else {
                continue;
            };
            last_by_path.insert(path, (change.uri, change.typ));
        }

        let mut messages = Vec::new();
        let mut appeared_or_vanished = Vec::new();
        for (path, (uri, typ)) in last_by_path {
            if typ == lsp_types::FileChangeType::DELETED {
                // A directory that was deleted takes every file loaded
                // under it, which the client reports no event for.
                let mut gone: Vec<PathBuf> = self
                    .project
                    .loaded_paths()
                    .filter(|loaded| loaded.starts_with(&path) && **loaded != path)
                    .map(Path::to_path_buf)
                    .collect();
                if path.extension().is_some_and(|e| e == "php") || self.project.is_loaded(&path) {
                    gone.push(path);
                } else {
                    appeared_or_vanished.push(path);
                }
                if gone.is_empty() {
                    continue;
                }
                for path in &gone {
                    self.open_uris.remove(path);
                    self.forget_findings(path);
                    self.latest_text.remove(path);
                    self.close_document(path);
                }
                let results = self.project.remove_files(&gone);
                messages.extend(self.publish_diagnostics_for(results));
                continue;
            }
            if !path.extension().is_some_and(|e| e == "php") && !self.project.is_loaded(&path) {
                if typ != lsp_types::FileChangeType::CHANGED {
                    appeared_or_vanished.push(path);
                }
                continue;
            }
            // Created and Changed both just need the path's current
            // on-disk content reflected - `update_file` already tolerates
            // a path with no prior entry (a brand-new file), so there is
            // no separate load-vs-update branch to make here. A read
            // failure (raced with a subsequent delete before this ran, a
            // transient permission error, ...) is skipped silently: a
            // later Deleted event, if any, cleans it up properly.
            let Ok(bytes) = self.read_file(&path) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            // No document version exists for an externally-observed
            // change (there is no didOpen/didChange behind it) - `None`
            // matches load_workspace's own "never opened" default.
            self.open_uris.insert(path.clone(), (uri, None));
            self.latest_text.insert(path.clone(), text.clone());
            let appeared = !self.project.is_loaded(&path);
            match self.project.update_file(&path, bytes.as_slice()) {
                Ok(results) => {
                    self.parse_failures.remove(&path);
                    messages.extend(self.publish_diagnostics_for(results));
                }
                Err(e) => {
                    messages.extend(self.parse_failed(&path, e, &bytes));
                    if appeared {
                        appeared_or_vanished.push(path);
                    }
                }
            }
        }
        if !appeared_or_vanished.is_empty() {
            let results = self.project.recheck_including(&appeared_or_vanished);
            messages.extend(self.publish_diagnostics_for(results));
        }
        messages
    }

    /// Keeps what the checks found by path in `last_diagnostics` and
    /// converts it into `publishDiagnostics` messages through the
    /// post-filters, resolving each path to whatever URI the client last
    /// used for it (a re-checked dependent might not itself be open in
    /// the editor - it still needs a fresh notification, using the
    /// workspace scan's own synthesized URI if nothing else ever opened
    /// it). Shared by `handle_open_or_change` and
    /// `remove_workspace_folders`'s own recheck step.
    fn publish_diagnostics_for(
        &mut self,
        results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    ) -> Vec<Message> {
        let paths: Vec<PathBuf> = results.keys().cloned().collect();
        self.last_diagnostics.extend(results);
        self.publish_shown(&paths, true)
    }

    /// `navigation::resolve_at` finds what symbol is under the cursor;
    /// this formats what the registry already knows about it into hover
    /// text. `None` for anything unresolved (a local variable's own
    /// read/declaration included - a real position, just not one hover
    /// has anything useful to say about) - a valid "no hover here" per
    /// the LSP spec, not an error.
    fn hover(&self, uri: &Uri, position: LspPosition) -> Option<Hover> {
        let path = uri_to_path(uri)?;
        let resolved =
            navigation::resolve_at(&self.project, &path, self.offset_at(&path, position)?)?;
        let text = self.format_hover(&resolved)?;
        Some(Hover {
            contents: HoverContents::Scalar(MarkedString::String(text)),
            range: None,
        })
    }

    fn format_hover(&self, resolved: &navigation::Resolved) -> Option<String> {
        let registry = self.project.registry();
        match resolved {
            navigation::Resolved::Function(fqn) => {
                Some(format_signature(fqn, registry.lookup_function(fqn)?))
            }
            navigation::Resolved::Class(fqn) => {
                let info = registry.classes().get(fqn.as_str())?;
                let kind = match info.kind() {
                    ClassKind::Class => "class",
                    ClassKind::Interface => "interface",
                    ClassKind::Trait => "trait",
                    ClassKind::Enum => "enum",
                };
                let mut text = format!("{kind} {fqn}");
                if !info.extends().is_empty() {
                    text.push_str(&format!(
                        " extends {}",
                        syntax::name::join(info.extends(), ", ")
                    ));
                }
                if !info.implements().is_empty() {
                    text.push_str(&format!(
                        " implements {}",
                        syntax::name::join(info.implements(), ", ")
                    ));
                }
                Some(text)
            }
            navigation::Resolved::Member(owner, name) => self.format_member_hover(owner, name),
            navigation::Resolved::Instance {
                receiver_span,
                member,
                scope,
                is_static,
                class,
                ..
            } => {
                let ty =
                    self.probe_receiver_type(*receiver_span, scope, *is_static, class.as_deref())?;
                let owners = resolve_member_owners(registry, &ty, member);
                self.format_member_hover_multi(&owners, member)
            }
            // A local variable's own declaration/read - no registry
            // signature exists for it, but its inferred type is
            // answerable via the identical throwaway-`Checker` probe
            // `Instance` above already uses ("what type is at this span,
            // in this scope" is the same question either way, not
            // something specific to a receiver).
            navigation::Resolved::VariableSelf {
                span,
                scope,
                is_static,
                class,
                ..
            }
            | navigation::Resolved::VariableRead {
                span,
                scope,
                is_static,
                class,
                ..
            } => {
                let ty = self.probe_receiver_type(*span, scope, *is_static, class.as_deref())?;
                Some(ty.to_string())
            }
        }
    }

    fn format_member_hover(&self, owner: &str, name: &str) -> Option<String> {
        let registry = self.project.registry();
        if let Some((_, sig)) = registry.lookup_method_with_owner(owner, name) {
            return Some(format_signature(&format!("{owner}::{name}"), sig));
        }
        if let Some((_, sig)) = registry.lookup_property_with_owner(owner, name) {
            return Some(format!("{owner}::${name}: {}", sig.ty()));
        }
        if let Some(constant_type) = registry.lookup_const_type(owner, name) {
            return Some(format!("{owner}::{name}: {constant_type}"));
        }
        None
    }

    /// `owners` is every class (from `resolve_member_owners`) that
    /// actually declares `name` - almost always exactly one, but a real
    /// `Union`/`Intersection` receiver can have more than one genuinely
    /// correct answer. Joins every one that resolves with `" | "` rather
    /// than picking an arbitrary first, the same "don't guess, show
    /// everything real" principle `definition`'s own multi-location
    /// response applies below.
    fn format_member_hover_multi(&self, owners: &[String], name: &str) -> Option<String> {
        let texts: Vec<String> = owners
            .iter()
            .filter_map(|o| self.format_member_hover(o, name))
            .collect();
        match texts.len() {
            0 => None,
            1 => Some(texts.into_iter().next().unwrap()),
            _ => Some(texts.join(" | ")),
        }
    }

    /// Runs the checker over just `scope` (a throwaway `Checker`, scoped
    /// to this one request - see `Checker::probe_span`'s own doc comment)
    /// to learn `receiver_span`'s own inferred type.
    /// Also the mechanism a receiver inside a nested closure/arrow-fn
    /// body resolves through: a `ProbeScope::Function` is always the
    /// enclosing *named* function/method (`lsp::navigation`'s own
    /// `current_function` never resets for a closure literal - see its
    /// doc comment for why), and `check_function`'s own ordinary
    /// recursion into that closure's body already reaches `probe_span`
    /// the same way a real `check-project` pass would. `ProbeScope::
    /// TopLevel` is the same idea one level up: a receiver/read at
    /// top-level script scope re-checks the whole file's own top-level
    /// statements via `check_program` instead.
    fn probe_receiver_type(
        &self,
        receiver_span: Span,
        scope: &navigation::ProbeScope,
        is_static: bool,
        class: Option<&str>,
    ) -> Option<Type> {
        let mut checker = Checker::new(self.project.registry())
            .with_sql(self.project.sql())
            .with_settings(self.project.check_settings())
            .with_bodies(self.project.bodies());
        checker.probe_span.set(Some(receiver_span));
        self.run_probe_scope(&mut checker, scope, is_static, class);
        checker.probe_hit.into_inner()
    }

    /// The local-variable-read counterpart of `probe_receiver_type`
    /// above - same throwaway-`Checker`-per-request mechanism, reading
    /// back `probe_var_provenance` instead of `probe_hit`. See
    /// `Checker::probe_var_provenance`'s own doc comment (`checker.rs`)
    /// for the full reaching-definitions design this reads the result
    /// of. Empty when nothing was tracked for this read at all
    /// (`Locals::track` was off - can't happen here, since this method
    /// itself is what turns it on via `probe_span` - or the name
    /// genuinely has no known declaration in scope, a possibility this
    /// project's own `variable.undefined` diagnostic would already have
    /// flagged).
    fn probe_variable_provenance(
        &self,
        span: Span,
        scope: &navigation::ProbeScope,
        is_static: bool,
        class: Option<&str>,
    ) -> Vec<Span> {
        let mut checker = Checker::new(self.project.registry())
            .with_sql(self.project.sql())
            .with_settings(self.project.check_settings())
            .with_bodies(self.project.bodies());
        checker.probe_span.set(Some(span));
        self.run_probe_scope(&mut checker, scope, is_static, class);
        checker
            .probe_var_provenance
            .into_inner()
            .map(|set| set.into_iter().collect())
            .unwrap_or_default()
    }

    /// Shared by both probe methods above: runs `checker` over whichever
    /// scope `navigation::resolve_at` identified - `check_function` for
    /// an ordinary named function/method, `check_program` for top-level
    /// script scope (`is_static`/`class` are meaningless there and
    /// ignored, matching how neither concept applies to top-level code).
    fn run_probe_scope(
        &self,
        checker: &mut Checker,
        scope: &navigation::ProbeScope,
        is_static: bool,
        class: Option<&str>,
    ) {
        match scope {
            navigation::ProbeScope::Function(function) => {
                checker.check_function(function, class, is_static, true);
            }
            navigation::ProbeScope::TopLevel(stmts) => {
                checker.check_program(stmts);
            }
        }
    }

    /// The `navigation::Resolved` symbol's own declaration site, from the
    /// registry's provenance - `None` when nothing resolved, or a
    /// symbol resolved by name but declared nowhere this project has
    /// loaded (an undefined reference - already flagged by an ordinary
    /// diagnostic, not this method's job to report again).
    fn definition(&self, uri: &Uri, position: LspPosition) -> Option<GotoDefinitionResponse> {
        let path = uri_to_path(uri)?;
        let resolved =
            navigation::resolve_at(&self.project, &path, self.offset_at(&path, position)?)?;
        // A local variable's own declaration site(s) are same-file and
        // never need a registry lookup at all, unlike every other
        // variant below.
        match &resolved {
            // The click's own position already is the answer - no probe
            // needed.
            navigation::Resolved::VariableSelf { span, .. } => {
                return Some(GotoDefinitionResponse::Scalar(self.location(&path, *span)));
            }
            // A real reaching-definitions probe - usually one location;
            // more than one exactly when the read is only reachable
            // through mutually exclusive branches that assign it
            // differently (see `Resolved::VariableRead`'s own doc
            // comment) - the same `Array`-vs-`Scalar` choice the
            // `Instance` arm below makes for a `Union`/`Intersection`
            // receiver.
            navigation::Resolved::VariableRead {
                span,
                scope,
                is_static,
                class,
                ..
            } => {
                let locs: Vec<Location> = self
                    .probe_variable_provenance(*span, scope, *is_static, class.as_deref())
                    .into_iter()
                    .map(|s| self.location(&path, s))
                    .collect();
                return match locs.len() {
                    0 => None,
                    1 => Some(GotoDefinitionResponse::Scalar(
                        locs.into_iter().next().unwrap(),
                    )),
                    _ => Some(GotoDefinitionResponse::Array(locs)),
                };
            }
            _ => {}
        }
        let registry = self.project.registry();
        match &resolved {
            navigation::Resolved::Class(fqn) => {
                let (target_path, span) = registry.class_declaration(fqn)?;
                Some(GotoDefinitionResponse::Scalar(
                    self.location(target_path, span),
                ))
            }
            navigation::Resolved::Function(fqn) => {
                let (target_path, span) = registry.function_declaration(fqn)?;
                Some(GotoDefinitionResponse::Scalar(
                    self.location(target_path, span),
                ))
            }
            navigation::Resolved::Member(owner, name) => {
                let (target_path, span) = registry.member_declaration(owner, name)?;
                Some(GotoDefinitionResponse::Scalar(
                    self.location(target_path, span),
                ))
            }
            // A `Union`/`Intersection` receiver can resolve `member`
            // against more than one real owner class - every one that
            // actually declares it becomes its own `Location`, not just
            // the first found. `GotoDefinitionResponse::Array` is
            // exactly LSP's own way to offer an editor more than one
            // real jump target (rendered as a picker), rather than this
            // server silently guessing which one the user meant.
            navigation::Resolved::Instance {
                receiver_span,
                member,
                scope,
                is_static,
                class,
                ..
            } => {
                let ty =
                    self.probe_receiver_type(*receiver_span, scope, *is_static, class.as_deref())?;
                let owners = resolve_member_owners(registry, &ty, member);
                let mut locs: Vec<Location> = Vec::new();
                for owner in &owners {
                    if let Some((target_path, span)) = registry.member_declaration(owner, member) {
                        locs.push(self.location(target_path, span));
                    }
                }
                match locs.len() {
                    0 => None,
                    1 => Some(GotoDefinitionResponse::Scalar(
                        locs.into_iter().next().unwrap(),
                    )),
                    _ => Some(GotoDefinitionResponse::Array(locs)),
                }
            }
            navigation::Resolved::VariableSelf { .. }
            | navigation::Resolved::VariableRead { .. } => {
                unreachable!("handled in the early match above")
            }
        }
    }

    /// `textDocument/typeDefinition` - jumps from an
    /// inferred `Type` to that type's own class declaration(s), reusing
    /// the identical throwaway-`Checker` probe/`type_to_classes` pair
    /// `format_hover`'s `Instance`/`VariableSelf`/`VariableRead` arms
    /// already use to answer "what type is at this span" - the only new
    /// work is resolving each class name the `Type` names to its own
    /// declaration in the registry, instead of formatting the `Type` as text.
    /// `Class`/`Function`/`Member` decline (`None`): none of them is a
    /// value with an inferred type to jump *from* (a class/function name
    /// reference already resolves to itself via ordinary
    /// `textDocument/definition`; a class member reference alone doesn't
    /// disambiguate a receiver type the way `Instance` does), matching
    /// `rename`'s own precedent of scoping a new capability narrower
    /// than every `Resolved` variant in its v1.
    fn type_definition(&self, uri: &Uri, position: LspPosition) -> Option<GotoDefinitionResponse> {
        let path = uri_to_path(uri)?;
        let resolved =
            navigation::resolve_at(&self.project, &path, self.offset_at(&path, position)?)?;
        let ty = match &resolved {
            navigation::Resolved::Instance {
                receiver_span,
                scope,
                is_static,
                class,
                ..
            } => self.probe_receiver_type(*receiver_span, scope, *is_static, class.as_deref())?,
            navigation::Resolved::VariableSelf {
                span,
                scope,
                is_static,
                class,
                ..
            }
            | navigation::Resolved::VariableRead {
                span,
                scope,
                is_static,
                class,
                ..
            } => self.probe_receiver_type(*span, scope, *is_static, class.as_deref())?,
            navigation::Resolved::Class(_)
            | navigation::Resolved::Function(_)
            | navigation::Resolved::Member(_, _) => return None,
        };
        let classes: Vec<String> = ty
            .object_class_names()
            .iter()
            .map(|c| c.to_string())
            .collect();
        let registry = self.project.registry();
        let mut locs: Vec<Location> = Vec::new();
        for class in &classes {
            if let Some((target_path, span)) = registry.class_declaration(class) {
                locs.push(self.location(target_path, span));
            }
        }
        match locs.len() {
            0 => None,
            1 => Some(GotoDefinitionResponse::Scalar(
                locs.into_iter().next().unwrap(),
            )),
            _ => Some(GotoDefinitionResponse::Array(locs)),
        }
    }

    fn document_symbol(&self, uri: &Uri) -> Option<DocumentSymbolResponse> {
        let path = uri_to_path(uri)?;
        let stmts = self.project.ast_of(&path)?;
        let stmts = &*stmts;
        let index = self.positions(&path)?;
        Some(DocumentSymbolResponse::Nested(
            document_symbol::document_symbols(stmts, index),
        ))
    }

    /// `textDocument/semanticTokens/full` - push-highlighting for every
    /// `@deprecated`/`@internal` usage site, independent of whether this
    /// project's own diagnostic gates for either are enabled. Always
    /// `Some` (even for zero flagged sites - `{data: []}`, never `null`,
    /// which the LSP spec treats as a different signal), matching
    /// `semantic_tokens_full`'s own always-return-Tokens contract.
    fn semantic_tokens_full(&self, uri: &Uri) -> Option<lsp_types::SemanticTokensResult> {
        let path = uri_to_path(uri)?;
        let tokens = self.project.semantic_tokens_for_file(&path);
        let index = self.positions(&path)?;
        Some(lsp_types::SemanticTokensResult::Tokens(
            lsp_types::SemanticTokens {
                result_id: None,
                data: semantic_tokens::encode(tokens, index),
            },
        ))
    }

    /// Maps a resolved symbol to every `use_sites::RefTarget` it answers
    /// for - more than one exactly for a `Union`/`Intersection` receiver
    /// (`Instance`), which can have more than one real declaring owner,
    /// mirroring `definition()`'s own `GotoDefinitionResponse::Array`
    /// handling of the identical case. A local variable
    /// (`VariableSelf`/`VariableRead`) answers for nothing - local-
    /// variable references/rename are out of this item's own v1 scope.
    fn ref_targets_for(&self, resolved: &navigation::Resolved) -> Vec<use_sites::RefTarget> {
        match resolved {
            navigation::Resolved::Class(fqn) => vec![use_sites::RefTarget::Class(fqn.clone())],
            navigation::Resolved::Function(fqn) => {
                vec![use_sites::RefTarget::Function(fqn.clone())]
            }
            navigation::Resolved::Member(owner, name) => {
                vec![member_target(self.project.registry(), owner.clone(), name)]
            }
            navigation::Resolved::Instance {
                receiver_span,
                member,
                scope,
                is_static,
                class,
            } => {
                let Some(ty) =
                    self.probe_receiver_type(*receiver_span, scope, *is_static, class.as_deref())
                else {
                    return Vec::new();
                };
                resolve_member_owners(self.project.registry(), &ty, member)
                    .into_iter()
                    .map(|owner| member_target(self.project.registry(), owner, member))
                    .collect()
            }
            navigation::Resolved::VariableSelf { .. }
            | navigation::Resolved::VariableRead { .. } => Vec::new(),
        }
    }

    /// `textDocument/references` - unions every `use_sites` site across
    /// every real target `ref_targets_for` resolves to, plus (when
    /// `include_declaration` is set) each target's own declaration site
    /// from the registry's provenance (already recorded at indexing, so no
    /// extra work). `ensure_member_index_built` runs unconditionally,
    /// even for a Class/Function-only query - see its own doc comment:
    /// the FIRST references/rename request of any kind pays this cost
    /// once, not just a member-targeted one.
    fn references(
        &mut self,
        uri: &Uri,
        position: LspPosition,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        let path = uri_to_path(uri)?;
        let resolved =
            navigation::resolve_at(&self.project, &path, self.offset_at(&path, position)?)?;
        let targets = self.ref_targets_for(&resolved);
        if targets.is_empty() {
            return None;
        }
        self.project.ensure_member_index_built();
        let mut locations: Vec<Location> = Vec::new();
        for target in &targets {
            for (p, span) in self.project.use_sites().sites(target) {
                locations.push(self.location(p, span));
            }
        }
        if include_declaration {
            let registry = self.project.registry();
            for target in &targets {
                let decl = match target {
                    use_sites::RefTarget::Class(fqn) => registry.class_declaration(fqn),
                    use_sites::RefTarget::Function(fqn)
                    | use_sites::RefTarget::UnqualifiedFunction(fqn) => {
                        registry.function_declaration(fqn)
                    }
                    use_sites::RefTarget::Member(owner, name) => {
                        registry.member_declaration(owner, name)
                    }
                };
                if let Some((p, span)) = decl {
                    locations.push(self.location(p, span));
                }
            }
        }
        // By file and place, not in the order the index took the files.
        locations.sort_by(|a, b| {
            (a.uri.as_str(), a.range.start, a.range.end).cmp(&(
                b.uri.as_str(),
                b.range.start,
                b.range.end,
            ))
        });
        (!locations.is_empty()).then_some(locations)
    }

    /// `textDocument/rename` - Class/Function targets only in v1 (see
    /// this item's own edge-case notes: member and local-variable rename
    /// are out of scope; a `Member` target's own `use_sites` spans cover
    /// a whole `e.span`-sized expression, not the member name's own
    /// precise sub-span, which a text-replacing rename can't safely use).
    /// Always includes the declaration site - unlike `references`, a
    /// rename must rewrite it regardless of any `includeDeclaration`-
    /// style flag (there is none in the LSP rename request).
    #[allow(clippy::mutable_key_type)]
    fn rename(
        &mut self,
        uri: &Uri,
        position: LspPosition,
        new_name: &str,
    ) -> Option<lsp_types::WorkspaceEdit> {
        let path = uri_to_path(uri)?;
        let resolved =
            navigation::resolve_at(&self.project, &path, self.offset_at(&path, position)?)?;
        let (target, decl) = match &resolved {
            navigation::Resolved::Class(fqn) => (
                use_sites::RefTarget::Class(fqn.clone()),
                self.project
                    .registry()
                    .class_declaration(fqn)
                    .map(|(p, s)| (p.to_path_buf(), s)),
            ),
            navigation::Resolved::Function(fqn) => (
                use_sites::RefTarget::Function(fqn.clone()),
                self.project
                    .registry()
                    .function_declaration(fqn)
                    .map(|(p, s)| (p.to_path_buf(), s)),
            ),
            _ => return None,
        };
        self.project.ensure_member_index_built();
        // A call that may reach another declared function too keeps its
        // name: renaming it would change which function it reaches.
        let sites: Vec<(&Path, Span)> = match &target {
            use_sites::RefTarget::Function(name) => self
                .project
                .use_sites()
                .function_references(name)
                .into_iter()
                .filter(|site| {
                    site.other.is_none_or(|other| {
                        self.project.registry().lookup_function(other).is_none()
                    })
                })
                .map(|site| (site.path, site.span))
                .collect(),
            _ => self.project.use_sites().sites(&target),
        };
        let mut changes: FxHashMap<Uri, Vec<lsp_types::TextEdit>> = FxHashMap::default();
        for (p, span) in sites {
            let (edit_uri, _) = self.uri_and_version_for(p);
            changes
                .entry(edit_uri)
                .or_default()
                .push(lsp_types::TextEdit {
                    range: self.lsp_range(p, span),
                    new_text: new_name.to_string(),
                });
        }
        if let Some((p, span)) = decl {
            let (edit_uri, _) = self.uri_and_version_for(&p);
            changes
                .entry(edit_uri)
                .or_default()
                .push(lsp_types::TextEdit {
                    range: self.lsp_range(&p, span),
                    new_text: new_name.to_string(),
                });
        }
        if changes.is_empty() {
            return None;
        }
        // Each file's edits in the order of the file, not in the order the
        // index took the files.
        for edits in changes.values_mut() {
            edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
        }
        Some(lsp_types::WorkspaceEdit {
            // `lsp_types` fixes the map type; entry order isn't observable.
            changes: Some(changes.into_iter().collect()),
            ..Default::default()
        })
    }

    /// Every same-file occurrence of the symbol under the cursor -
    /// `navigation::collect_all` runs the identical resolution walk
    /// `resolve_at` uses for hover/definition, just against every span in
    /// the file instead of stopping at the first cursor-containing match,
    /// then `same_symbol` filters it down. Deduped and sorted since a
    /// `MethodCall`/`PropertyAccess` on `$this` can independently resolve
    /// to the same `(owner, member)` `Resolved::Member` as a sibling
    /// method's own declaration, and `Instance`'s owner-normalization can
    /// likewise coincide with more than one syntactically-distinct
    /// candidate.
    fn document_highlight(
        &self,
        uri: &Uri,
        position: LspPosition,
    ) -> Option<Vec<DocumentHighlight>> {
        let path = uri_to_path(uri)?;
        let target =
            navigation::resolve_at(&self.project, &path, self.offset_at(&path, position)?)?;
        let mut spans: Vec<Span> = navigation::collect_all(&self.project, &path)
            .into_iter()
            .filter(|(_, r)| self.same_symbol(&target, r))
            .map(|(span, _)| span)
            .collect();
        spans.sort_by_key(|s| (s.start, s.end));
        spans.dedup();
        let highlights: Vec<DocumentHighlight> = spans
            .into_iter()
            .map(|span| DocumentHighlight {
                range: self.lsp_range(&path, span),
                kind: None,
            })
            .collect();
        (!highlights.is_empty()).then_some(highlights)
    }

    /// Whether `target` and `candidate` refer to the same real symbol -
    /// exact FQN equality for `Class`/`Function`, name-plus-syntactic-
    /// scope equality for a local variable (see `Resolved::VariableSelf`/
    /// `VariableRead`'s own `var_scope` doc comment), and declaring-owner
    /// equality (normalized via `member_owners_of`, since a `Member`'s
    /// own `owner` field is the syntactic reference site's class, not
    /// necessarily the one that actually declares it) for a member/
    /// instance access - checked by name first as a cheap pre-filter
    /// before the owner-normalizing work, which for `Instance` includes a
    /// real checker probe.
    fn same_symbol(&self, target: &navigation::Resolved, candidate: &navigation::Resolved) -> bool {
        use navigation::Resolved::{Class, Function, Instance, Member, VariableRead, VariableSelf};
        match (target, candidate) {
            (Class(a), Class(b)) => a == b,
            (Function(a), Function(b)) => a == b,
            (
                VariableSelf {
                    name: a,
                    var_scope: sa,
                    ..
                }
                | VariableRead {
                    name: a,
                    var_scope: sa,
                    ..
                },
                VariableSelf {
                    name: b,
                    var_scope: sb,
                    ..
                }
                | VariableRead {
                    name: b,
                    var_scope: sb,
                    ..
                },
            ) => a == b && sa == sb,
            (Member(..) | Instance { .. }, Member(..) | Instance { .. }) => {
                if member_name_hint(target) != member_name_hint(candidate) {
                    return false;
                }
                let Some((owners_a, _)) = self.member_owners_of(target) else {
                    return false;
                };
                let Some((owners_b, _)) = self.member_owners_of(candidate) else {
                    return false;
                };
                owners_a.iter().any(|o| owners_b.contains(o))
            }
            _ => false,
        }
    }

    /// Normalizes a `Member`/`Instance` down to every real declaring
    /// owner (plural only for `Instance` on a `Union`/`Intersection`
    /// receiver) plus the member name - `Member`'s own `owner` field is
    /// whatever class the reference was syntactically written against
    /// (`self`/a literal name/`$this`'s own `current_class`), not
    /// necessarily the one that actually declares the member (an
    /// inherited method/property/const), so this re-resolves it through
    /// the same `lookup_*_with_owner` calls `format_member_hover` already
    /// uses - without this, a call on a subclass and the same method's
    /// own declaration on its parent would compare as different owners
    /// and never highlight together.
    fn member_owners_of<'a>(&self, r: &'a navigation::Resolved) -> Option<(Vec<String>, &'a str)> {
        let registry = self.project.registry();
        match r {
            navigation::Resolved::Member(owner, name) => {
                let normalized = registry
                    .lookup_method_with_owner(owner, name)
                    .map(|(o, _)| o.to_string())
                    .or_else(|| {
                        registry
                            .lookup_property_with_owner(owner, name)
                            .map(|(o, _)| o.to_string())
                    })
                    .or_else(|| {
                        registry
                            .lookup_const_with_owner(owner, name)
                            .map(|(o, _)| o.to_string())
                    })
                    .unwrap_or_else(|| owner.clone());
                Some((vec![normalized], name.as_str()))
            }
            navigation::Resolved::Instance {
                receiver_span,
                member,
                scope,
                is_static,
                class,
            } => {
                let ty =
                    self.probe_receiver_type(*receiver_span, scope, *is_static, class.as_deref())?;
                Some((
                    resolve_member_owners(registry, &ty, member),
                    member.as_str(),
                ))
            }
            _ => None,
        }
    }

    /// `textDocument/inlayHint` - serves `inlay_hint_cache` when `path`'s
    /// current `file_versions` entry matches the cached one, recomputing
    /// via `inlay_hints::collect` (one `check_program` pass) on a miss.
    /// See `inlay_hints`'s own module doc comment for why the cache
    /// exists at all.
    fn inlay_hints(&mut self, uri: &Uri, range: LspRange) -> Option<Vec<lsp_types::InlayHint>> {
        let path = uri_to_path(uri)?;
        let version = *self.file_versions.get(&path).unwrap_or(&0);
        let cached = match self.inlay_hint_cache.get(&path) {
            Some((cached_version, params, locals)) if *cached_version == version => {
                (params.clone(), locals.clone())
            }
            _ => {
                let (params, locals) = inlay_hints::collect(&self.project, &path)?;
                self.inlay_hint_cache
                    .insert(path.clone(), (version, params.clone(), locals.clone()));
                (params, locals)
            }
        };
        let hints =
            inlay_hints::filter_to_range(&cached.0, &cached.1, range, self.positions(&path)?);
        (!hints.is_empty()).then_some(hints)
    }

    /// Project-wide fuzzy symbol search - see `workspace_symbol::
    /// workspace_symbols`'s own doc comment for ranking/scope details
    /// and its documented member-collision limitation.
    #[allow(deprecated)] // SymbolInformation::deprecated: no replacement field exists yet
    fn workspace_symbol(&self, query: &str) -> Vec<SymbolInformation> {
        workspace_symbol::workspace_symbols(&self.project, query)
            .into_iter()
            .map(|m| SymbolInformation {
                name: m.name,
                kind: m.kind,
                tags: None,
                deprecated: None,
                location: self.location(m.path, m.span),
                container_name: m.container_name,
            })
            .collect()
    }

    /// An active-parameter tooltip for the call the cursor is currently
    /// inside the parens of - see `signature_help::resolve_at`'s own
    /// module doc comment for the claim-and-clear walker this reads.
    fn signature_help(&self, uri: &Uri, position: LspPosition) -> Option<SignatureHelp> {
        let path = uri_to_path(uri)?;
        let pos = self.offset_at(&path, position)?;
        let help = signature_help::resolve_at(&self.project, &path, pos)?;
        self.build_signature_help(help)
    }

    /// Resolves `help.target` into one or more real `FunctionReflection`s (more
    /// than one only for `Instance` on a `Union`/`Intersection`
    /// receiver, the same multi-owner case `format_member_hover_multi`
    /// already handles for hover) and builds the LSP response.
    /// `activeParameter` is set on every returned `SignatureInformation`
    /// (not just the top-level `SignatureHelp.activeParameter`, which
    /// only ever describes signature 0) so a client honors it correctly
    /// regardless of which signature it's currently displaying.
    fn build_signature_help(&self, help: signature_help::SigHelp) -> Option<SignatureHelp> {
        let registry = self.project.registry();
        let signatures: Vec<(String, &FunctionReflection)> = match &help.target {
            signature_help::Target::Function(fqn) => {
                vec![(fqn.clone(), registry.lookup_function(fqn)?)]
            }
            signature_help::Target::Constructor(class) => {
                let (owner, sig) = registry.lookup_method_with_owner(class, "__construct")?;
                vec![(format!("{owner}::__construct"), sig)]
            }
            signature_help::Target::Member(owner, name) => {
                let (real_owner, sig) = registry.lookup_method_with_owner(owner, name)?;
                vec![(format!("{real_owner}::{name}"), sig)]
            }
            signature_help::Target::Instance {
                receiver_span,
                member,
                scope,
                is_static,
                class,
            } => {
                let ty =
                    self.probe_receiver_type(*receiver_span, scope, *is_static, class.as_deref())?;
                let owners = resolve_member_owners(registry, &ty, member);
                let sigs: Vec<(String, &FunctionReflection)> = owners
                    .iter()
                    .filter_map(|owner| {
                        registry
                            .lookup_method_with_owner(owner, member)
                            .map(|(real_owner, sig)| (format!("{real_owner}::{member}"), sig))
                    })
                    .collect();
                if sigs.is_empty() {
                    return None;
                }
                sigs
            }
        };

        let signature_infos: Vec<SignatureInformation> = signatures
            .into_iter()
            .map(|(name, sig)| {
                let active_parameter = if sig.params().is_empty() {
                    None
                } else {
                    Some(help.active_parameter.min(sig.params().len() - 1) as u32)
                };
                SignatureInformation {
                    label: format_signature(&name, sig),
                    documentation: None,
                    parameters: Some(
                        sig.params()
                            .iter()
                            .map(|p| ParameterInformation {
                                label: ParameterLabel::Simple(param_label(p)),
                                documentation: None,
                            })
                            .collect(),
                    ),
                    active_parameter,
                }
            })
            .collect();

        let active_parameter = signature_infos.first().and_then(|s| s.active_parameter);
        Some(SignatureHelp {
            signatures: signature_infos,
            active_signature: Some(0),
            active_parameter,
        })
    }

    /// See `completion`'s own module doc comment for the two mechanisms
    /// this dispatches between. `latest_text` (falling back to
    /// `Project::source` for a workspace file never `didOpen`ed) is the
    /// LIVE buffer text - `Project::ast`'s last-good AST is used both as
    /// the member-completion probe's own scope source and, for a bare
    /// trigger, as the import-resolution context `enumerate_bare` needs;
    /// using the stale AST for that (rather than re-deriving a `use`
    /// context from the live, possibly-unparseable buffer) is the same
    /// "last-good AST" precedent this whole request already leans on.
    fn completion(&self, uri: &Uri, position: LspPosition) -> Option<CompletionResponse> {
        let path = uri_to_path(uri)?;
        let text = self
            .latest_text
            .get(&path)
            .cloned()
            .or_else(|| self.project.source(&path).map(str::to_string))?;
        let stmts = self.project.ast_of(&path)?;
        let stmts = &*stmts;
        let tokens = completion::tokenize(&text);
        let live_index = LineIndex::new(text.as_bytes());
        let stale_index = self.project.line_index(&path)?;
        let cursor = completion::Cursor {
            live: live_index.offset_in_clamped(position.line, position.character, self.unit),
            live_index: &live_index,
            stale: stale_index.offset_in_clamped(position.line, position.character, self.unit),
            stale_index,
        };
        match completion::build_member_probe(&text, &tokens, cursor, stmts, self.project.registry())
        {
            Some(completion::MemberProbe::Resolved {
                ty,
                is_static,
                class,
            }) => Some(CompletionResponse::Array(completion::enumerate_members(
                self.project.registry(),
                &ty,
                is_static,
                class.as_deref(),
            ))),
            Some(completion::MemberProbe::NeedsProbe {
                scope,
                scope_is_static,
                class,
                probe_span,
                is_static,
            }) => {
                let ty = self.probe_receiver_type(
                    probe_span,
                    &scope,
                    scope_is_static,
                    class.as_deref(),
                )?;
                Some(CompletionResponse::Array(completion::enumerate_members(
                    self.project.registry(),
                    &ty,
                    is_static,
                    class.as_deref(),
                )))
            }
            None => Some(CompletionResponse::Array(completion::enumerate_bare(
                &self.project,
                &path,
                stmts,
                cursor.stale,
                self.unit,
            ))),
        }
    }

    /// Quick fixes for whatever diagnostics in `path` overlap `range` and
    /// carry a `DiagnosticFix`, of those the post-filters show. Reads
    /// `self.last_diagnostics` rather than re-running the checker - see
    /// that field's own doc comment for why. A cache miss (a workspace
    /// file requested before any `didOpen`/`didChange`/scan ever
    /// populated it) is a one-time, self-healing fallback: computed once
    /// via `Project::check_file`, then cached.
    fn code_actions(&mut self, uri: &Uri, range: LspRange) -> Option<CodeActionResponse> {
        let path = uri_to_path(uri)?;
        if !self.last_diagnostics.contains_key(&path) {
            let found = self.project.check_file(&path);
            self.last_diagnostics.insert(path.clone(), found);
        }
        let diagnostics = self
            .apply_post_filters(self.held_findings([&path]))
            .remove(&path)?;
        let source = self.project.source(&path);
        let positions = self.positions(&path)?;
        Some(
            diagnostics
                .iter()
                .filter(|d| {
                    d.fix.is_some()
                        && ranges_overlap(
                            range_to_lsp(Some(positions.index), positions.unit, d.span),
                            range,
                        )
                })
                .filter_map(|d| diagnostic_fix_to_code_action(uri, d, source, positions))
                .collect(),
        )
    }

    fn location(&self, target_path: &Path, span: Span) -> Location {
        let (target_uri, _version) = self.uri_and_version_for(target_path);
        Location {
            uri: target_uri,
            range: self.lsp_range(target_path, span),
        }
    }

    /// The byte offset of `position` in the loaded file `path`.
    fn offset_at(&self, path: &Path, position: LspPosition) -> Option<u32> {
        offset_from_lsp(self.project.line_index(path)?, self.unit, position)
    }

    /// The LSP range of `span` in the loaded file `path`; empty when the
    /// file isn't loaded.
    fn lsp_range(&self, path: &Path, span: Span) -> LspRange {
        self.project
            .line_index(path)
            .map_or_else(LspRange::default, |index| {
                Positions {
                    index,
                    unit: self.unit,
                }
                .range(span)
            })
    }

    /// `Server::reload_configuration` for this folder: re-resolves its
    /// configuration, `enable` being the client's setting now and
    /// `was_enabled` what it was, and applies and republishes what changed.
    fn reload_configuration(
        &mut self,
        enable: bool,
        was_enabled: bool,
        inputs: &ConfigInputs,
    ) -> Vec<Message> {
        let Some(root) = self.root.clone() else {
            return Vec::new();
        };
        let workspace = inputs.open(&root);
        let resolved = workspace.config;
        if enable == was_enabled && self.last_applied_config.as_ref() == Some(&resolved) {
            return Vec::new();
        }

        let file_set_changed = self
            .last_applied_config
            .as_ref()
            .is_some_and(|prev| prev.file_set != resolved.file_set);
        let checks_unchanged = self
            .last_applied_config
            .as_ref()
            .is_some_and(|prev| prev.settings == resolved.settings);

        let mut messages = self.configuration_problems(&resolved);
        if checks_unchanged {
            self.apply_report_config(resolved);
        } else {
            self.apply_resolved_config(resolved);
        }
        let previous = self.adopt_session(workspace.session);
        if file_set_changed {
            messages.extend(self.reload_file_set(previous));
        }

        messages.extend(if checks_unchanged && !file_set_changed {
            self.republish_last_diagnostics(enable)
        } else {
            self.republish_known_files(enable)
        });
        messages
    }

    /// Makes the loaded files what the workspace session lists now: the
    /// files the `previous` session listed and this one does not are
    /// removed, those it lists that are not loaded are loaded, and those
    /// whose role is another are loaded again. A file the client has opened
    /// stays loaded.
    fn reload_file_set(&mut self, previous: Option<Session>) -> Vec<Message> {
        let mut messages = Vec::new();
        let Some(session) = &self.session else {
            return messages;
        };
        let (analysed, context) = session.listing();
        let wanted: FxHashMap<&PathBuf, bool> = context
            .iter()
            .map(|path| (path, false))
            .chain(analysed.iter().map(|path| (path, true)))
            .collect();
        let analysed_loaded: FxHashSet<PathBuf> =
            self.project.analyzed_paths().into_iter().collect();
        let previously_listed: FxHashSet<PathBuf> = previous
            .map(|previous| {
                let (analysed, context) = previous.listing();
                analysed.into_iter().chain(context).collect()
            })
            .unwrap_or_default();
        let mut gone: Vec<PathBuf> = Vec::new();
        let mut again: FxHashSet<PathBuf> = FxHashSet::default();
        for path in self.project.loaded_paths() {
            match wanted.get(&path.to_path_buf()) {
                Some(&is_analysed) => {
                    if is_analysed != analysed_loaded.contains(path) {
                        gone.push(path.to_path_buf());
                        again.insert(path.to_path_buf());
                    }
                }
                None => {
                    if previously_listed.contains(path) && !self.file_versions.contains_key(path) {
                        gone.push(path.to_path_buf());
                    }
                }
            }
        }
        let to_load: Vec<(PathBuf, bool)> = context
            .iter()
            .map(|path| (path, false))
            .chain(analysed.iter().map(|path| (path, true)))
            .filter(|(path, _)| !self.project.is_loaded(path) || again.contains(*path))
            .map(|(path, is_analysed)| (path.clone(), is_analysed))
            .collect();
        if !gone.is_empty() {
            for path in &gone {
                self.open_uris.remove(path);
                self.forget_findings(path);
                self.latest_text.remove(path);
                self.close_document(path);
            }
            let results = self.project.remove_files(&gone);
            messages.extend(self.publish_diagnostics_for(results));
        }
        self.load_files(&to_load, &mut messages);
        self.project.restore_load_order();
        self.project.finish_loading();
        messages
    }
}

impl Server {
    fn new(version: &str, config_schema: config::Schema) -> Self {
        let (loaded_tx, loaded_rx) = crossbeam_channel::unbounded();
        Server {
            folders: Vec::new(),
            unrooted: Workspace::new(None, Unit::default()),
            pool: crate::project::worker_pool(),
            unit: Unit::default(),
            shutdown_received: false,
            pending_requests: lsp_server::ReqQueue::default(),
            // `Incoming<I>` has no public constructor or `Default` impl
            // of its own (its one field is private to `lsp-server`; only
            // `ReqQueue`'s manual `Default` impl can see it) - built via
            // a throwaway `ReqQueue` whose `.outgoing` half (typed `()`,
            // never touched) is immediately dropped.
            pending_incoming_requests: lsp_server::ReqQueue::<Arc<AtomicBool>, ()>::default()
                .incoming,
            pending_workspace_scan: None,
            loading: Vec::new(),
            loaded_tx,
            loaded_rx,
            scan_progress: None,
            config_path_override: None,
            enabled: true,
            supports_configuration_pull: false,
            supports_watched_files: false,
            version: version.to_string(),
            config_schema: Arc::new(config_schema),
        }
    }

    /// Sets what a column of a position counts, in every workspace.
    fn set_unit(&mut self, unit: Unit) {
        self.unit = unit;
        for workspace in self.workspaces_mut() {
            workspace.unit = unit;
        }
    }

    /// Every workspace: the folders', then the one of the documents no
    /// folder contains.
    fn workspaces(&self) -> impl Iterator<Item = &Workspace> {
        self.folders.iter().chain(std::iter::once(&self.unrooted))
    }

    fn workspaces_mut(&mut self) -> impl Iterator<Item = &mut Workspace> {
        self.folders
            .iter_mut()
            .chain(std::iter::once(&mut self.unrooted))
    }

    /// The folder `path` belongs to: the deepest that contains it, `None`
    /// when no folder does.
    fn folder_of(&self, path: &Path) -> Option<usize> {
        self.folders
            .iter()
            .enumerate()
            .filter_map(|(index, workspace)| {
                let root = workspace.root.as_deref()?;
                path.starts_with(root)
                    .then(|| (root.components().count(), index))
            })
            .max()
            .map(|(_, index)| index)
    }

    /// Who the document at `path` belongs to: the deepest of the folders
    /// and of those loading that contains it.
    fn owner_of(&self, path: &Path) -> Owner {
        let depth = |root: &Path| root.components().count();
        let folder = self.folder_of(path).map(|index| {
            let root = self.folders[index].root.as_deref().unwrap_or(Path::new(""));
            (depth(root), index)
        });
        let loading = self
            .loading
            .iter()
            .enumerate()
            .filter(|(_, loading)| path.starts_with(&loading.root))
            .map(|(index, loading)| (depth(&loading.root), index))
            .max();
        match (folder, loading) {
            (Some((folder_depth, _)), Some((loading_depth, index)))
                if loading_depth > folder_depth =>
            {
                Owner::Loading(index)
            }
            (None, Some((_, index))) => Owner::Loading(index),
            (Some((_, index)), _) => Owner::Folder(index),
            (None, None) => Owner::Unrooted,
        }
    }

    /// The workspace the document at `path` belongs to.
    fn workspace_of(&mut self, path: &Path) -> &mut Workspace {
        match self.folder_of(path) {
            Some(index) => &mut self.folders[index],
            None => &mut self.unrooted,
        }
    }

    /// Runs `request` on the workspace the document at `uri` belongs to;
    /// `None` when `uri` names no file.
    fn on_document<T>(
        &mut self,
        uri: &Uri,
        request: impl FnOnce(&mut Workspace) -> Option<T>,
    ) -> Option<T> {
        let path = uri_to_path(uri)?;
        request(self.workspace_of(&path))
    }

    /// Tells each workspace which folders, loading ones included, are
    /// nested inside it, after the folders changed.
    fn refresh_nesting(&mut self) {
        let roots: Vec<PathBuf> = self
            .folders
            .iter()
            .filter_map(|workspace| workspace.root.clone())
            .chain(self.loading.iter().map(|loading| loading.root.clone()))
            .collect();
        for workspace in self.workspaces_mut() {
            workspace.nested_roots = roots
                .iter()
                .filter(|root| match &workspace.root {
                    Some(own) => root.starts_with(own) && *root != own,
                    None => true,
                })
                .cloned()
                .collect();
        }
    }

    /// Starts building a workspace on the pool for each of `roots` that is
    /// not a folder or loading yet, each with its own session, project and
    /// configuration. Called from `"initialized"` (the
    /// `pending_workspace_scan` roots resolved at `"initialize"`) and from
    /// `"workspace/didChangeWorkspaceFolders"`'s own `added` array; the
    /// message loop installs each workspace as the pool finishes it
    /// (`install_loaded`).
    fn start_loading(&mut self, roots: &[PathBuf]) {
        for root in roots {
            if self.is_folder_or_loading(root) {
                continue;
            }
            self.loading.push(LoadingFolder {
                root: root.clone(),
                documents: FxHashMap::default(),
                watched: Vec::new(),
            });
            let root = root.clone();
            let unit = self.unit;
            let path_override = self.config_path_override.clone();
            let schema = Arc::clone(&self.config_schema);
            let pool = Arc::clone(&self.pool);
            let loaded_tx = self.loaded_tx.clone();
            self.pool.spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let inputs = ConfigInputs {
                        path_override: path_override.as_deref(),
                        schema: &schema,
                        pool: &pool,
                    };
                    Workspace::load(root.clone(), unit, &inputs)
                }))
                .map_err(crate::project::panic_message);
                let _ = loaded_tx.send(LoadedFolder { root, outcome });
            });
        }
        self.refresh_nesting();
    }

    fn is_folder_or_loading(&self, root: &Path) -> bool {
        self.folders
            .iter()
            .any(|workspace| workspace.root.as_deref() == Some(root))
            || self.loading.iter().any(|loading| loading.root == root)
    }

    /// Starts the scan of the workspace folders `"initialize"` named,
    /// opening the progress report the client asked for, which ends with
    /// the last of them installed.
    fn start_scan(&mut self, scan: PendingWorkspaceScan) -> Vec<Message> {
        let mut messages = Vec::new();
        let Some(token) = scan.progress_token else {
            self.start_loading(&scan.roots);
            return messages;
        };
        messages.push(progress_notification(
            token.clone(),
            WorkDoneProgress::Begin(WorkDoneProgressBegin {
                title: "phpstan-rs: scanning workspace".to_string(),
                cancellable: Some(false),
                message: None,
                percentage: None,
            }),
        ));
        self.start_loading(&scan.roots);
        let roots: FxHashSet<PathBuf> = scan
            .roots
            .into_iter()
            .filter(|root| self.loading.iter().any(|loading| &loading.root == root))
            .collect();
        if roots.is_empty() {
            messages.push(scan_progress_end(token));
        } else {
            self.scan_progress = Some((token, roots));
        }
        messages
    }

    /// Installs the workspace the pool built for a folder: the documents
    /// the client opened or edited meanwhile are applied to it, the
    /// watched-file events replayed, and what its checks find is
    /// published.
    fn install_loaded(&mut self, loaded: LoadedFolder) -> Vec<Message> {
        let Some(position) = self
            .loading
            .iter()
            .position(|loading| loading.root == loaded.root)
        else {
            return Vec::new();
        };
        let pending = self.loading.remove(position);
        let mut messages = Vec::new();
        match loaded.outcome {
            Ok((workspace, load_messages)) => {
                messages.extend(load_messages);
                self.folders.push(workspace);
                let added = self.folders.len() - 1;
                self.refresh_nesting();
                messages.extend(self.folders[added].publish_workspace_diagnostics());
                messages.extend(self.folders[added].handle_watched_files_changed(pending.watched));
                let mut documents = self.take_open_documents_for(added);
                documents.extend(pending.documents.into_values());
                for document in documents {
                    messages.extend(self.folders[added].handle_open_or_change(
                        &document.uri,
                        &document.text,
                        document.version,
                    ));
                }
            }
            Err(failure) => {
                self.refresh_nesting();
                messages.push(show_message_notification(
                    MessageType::ERROR,
                    format!(
                        "phpstan-rs: loading {} failed: {failure}",
                        loaded.root.display()
                    ),
                ));
            }
        }
        if let Some((_, roots)) = &mut self.scan_progress {
            roots.remove(&loaded.root);
            if roots.is_empty()
                && let Some((token, _)) = self.scan_progress.take()
            {
                messages.push(scan_progress_end(token));
            }
        }
        messages
    }

    /// Installs every workspace still loading, waiting for the pool.
    fn finish_loads(&mut self) -> Vec<Message> {
        let mut messages = Vec::new();
        while !self.loading.is_empty() {
            let Ok(loaded) = self.loaded_rx.recv() else {
                break;
            };
            messages.extend(self.install_loaded(loaded));
        }
        messages
    }

    /// Loads the workspaces of `roots` and installs them: `start_loading`,
    /// then `finish_loads`.
    #[cfg(test)]
    fn add_workspace_folders(&mut self, roots: &[PathBuf]) -> Vec<Message> {
        self.start_loading(roots);
        self.finish_loads()
    }

    /// Takes the documents open in another workspace that belong to the
    /// folder `added` out of it.
    fn take_open_documents_for(&mut self, added: usize) -> Vec<OpenDocument> {
        let moving: Vec<PathBuf> = self
            .workspaces()
            .filter(|workspace| workspace.root != self.folders[added].root)
            .flat_map(|workspace| workspace.open_documents.iter())
            .filter(|path| self.folder_of(path) == Some(added))
            .cloned()
            .collect();
        moving
            .iter()
            .filter_map(|path| {
                self.workspaces_mut()
                    .find_map(|workspace| workspace.take_document(path))
            })
            .collect()
    }

    /// Drops the workspace of each of `roots`, with its project, and
    /// publishes an empty `publishDiagnostics` for each file it had
    /// published for - otherwise a document whose backing folder just
    /// vanished would keep showing stale squiggles indefinitely, since
    /// removing a workspace folder is not itself a `textDocument/didClose`.
    fn remove_workspace_folders(&mut self, roots: &[PathBuf]) -> Vec<Message> {
        let mut messages = self.finish_loads();
        self.folders.retain(|workspace| {
            let removed = workspace
                .root
                .as_ref()
                .is_some_and(|root| roots.contains(root));
            if removed {
                messages.extend(workspace.clear_published());
            }
            !removed
        });
        self.refresh_nesting();
        messages
    }

    fn handle_open_or_change(
        &mut self,
        uri: &Uri,
        text: &str,
        version: Option<i32>,
    ) -> Vec<Message> {
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        if let Owner::Loading(index) = self.owner_of(&path) {
            self.loading[index].record_document(&path, uri, text, version);
            return Vec::new();
        }
        self.workspace_of(&path)
            .handle_open_or_change(uri, text, version)
    }

    fn handle_debounced_change(
        &mut self,
        path: &Path,
        uri: &Uri,
        text: &str,
        version: i32,
        should_abort: impl Fn() -> bool + Sync,
    ) -> Vec<Message> {
        if let Owner::Loading(index) = self.owner_of(path) {
            self.loading[index].record_document(path, uri, text, Some(version));
            return Vec::new();
        }
        self.workspace_of(path)
            .handle_debounced_change(path, uri, text, version, should_abort)
    }

    /// Notes that the client closed the document at `path`.
    fn close_document(&mut self, path: &Path) {
        match self.owner_of(path) {
            Owner::Loading(index) => {
                self.loading[index].documents.remove(path);
            }
            _ => self.workspace_of(path).close_document(path),
        }
    }

    /// Runs the whole-program checks of every workspace again.
    fn recheck_whole_program(&mut self) -> Vec<Message> {
        let enabled = self.enabled;
        self.workspaces_mut()
            .flat_map(|workspace| workspace.recheck_whole_program(enabled))
            .collect()
    }

    /// Project-wide fuzzy symbol search, over every workspace.
    fn workspace_symbol(&self, query: &str) -> Vec<SymbolInformation> {
        self.workspaces()
            .flat_map(|workspace| workspace.workspace_symbol(query))
            .collect()
    }

    /// Routes a `workspace/didChangeWatchedFiles` batch to the workspace
    /// each changed path belongs to.
    fn handle_watched_files_changed(&mut self, changes: Vec<lsp_types::FileEvent>) -> Vec<Message> {
        let mut by_workspace: Vec<Vec<lsp_types::FileEvent>> =
            (0..=self.folders.len()).map(|_| Vec::new()).collect();
        for change in changes {
            let Some(path) = uri_to_path(&change.uri) else {
                continue;
            };
            match self.owner_of(&path) {
                Owner::Loading(index) => self.loading[index].watched.push(change),
                Owner::Folder(index) => by_workspace[index].push(change),
                Owner::Unrooted => by_workspace[self.folders.len()].push(change),
            }
        }
        self.workspaces_mut()
            .zip(by_workspace)
            .flat_map(|(workspace, changes)| workspace.handle_watched_files_changed(changes))
            .collect()
    }

    /// Sends a server-initiated JSON-RPC request and registers its id
    /// for later correlation with the client's reply in
    /// `handle_response`. `R` fixes `params` to the shape `R::METHOD`
    /// expects at compile time.
    fn send_request<R: lsp_types::request::Request>(&mut self, params: R::Params) -> Message {
        Message::Request(self.pending_requests.outgoing.register(
            R::METHOD.to_string(),
            params,
            R::METHOD,
        ))
    }

    /// Builds (and registers) a `workspace/configuration` request asking
    /// for the `"phpstan-rs"` section - `None` if the client never
    /// advertised `workspace.configuration` support at `"initialize"`, in
    /// which case there is nothing to send and the caller should fall
    /// back to re-resolving from disk instead.
    fn request_configuration_pull(&mut self) -> Option<Message> {
        if !self.supports_configuration_pull {
            return None;
        }
        Some(
            self.send_request::<lsp_types::request::WorkspaceConfiguration>(
                lsp_types::ConfigurationParams {
                    items: vec![lsp_types::ConfigurationItem {
                        scope_uri: None,
                        section: Some("phpstan-rs".to_string()),
                    }],
                },
            ),
        )
    }

    /// Builds (and registers) a `client/registerCapability` request
    /// asking the client to watch every `.php` file and report changes
    /// via `workspace/didChangeWatchedFiles` - `None` if the client never
    /// advertised `workspace.didChangeWatchedFiles.dynamicRegistration`
    /// support at `"initialize"`. Of every other file and directory, only
    /// its creation and deletion are asked for: an `include` can name any
    /// file, and whether it is there is all its check reads of it. Not a
    /// change to `composer.{json,lock}`: LSP mode has no composer-aware
    /// reload path to route one through yet.
    fn request_watched_files_registration(&mut self) -> Option<Message> {
        if !self.supports_watched_files {
            return None;
        }
        Some(self.send_request::<lsp_types::request::RegisterCapability>(
            lsp_types::RegistrationParams {
                registrations: vec![lsp_types::Registration {
                        id: "phpstan-rs/watchedFiles".to_string(),
                        method: "workspace/didChangeWatchedFiles".to_string(),
                        register_options: Some(
                            serde_json::to_value(
                                lsp_types::DidChangeWatchedFilesRegistrationOptions {
                                    watchers: vec![
                                        lsp_types::FileSystemWatcher {
                                            glob_pattern: lsp_types::GlobPattern::String(
                                                "**/*.php".to_string(),
                                            ),
                                            kind: None,
                                        },
                                        lsp_types::FileSystemWatcher {
                                            glob_pattern: lsp_types::GlobPattern::String(
                                                "**/*".to_string(),
                                            ),
                                            kind: Some(
                                                lsp_types::WatchKind::Create
                                                    | lsp_types::WatchKind::Delete,
                                            ),
                                        },
                                    ],
                                },
                            )
                            .unwrap(),
                        ),
                    }],
            },
        ))
    }

    /// The `workspace/configuration` response counterpart to
    /// `request_configuration_pull` - parses `resp`'s single item back
    /// into `settings::PhpstanRsSettings` and feeds it into
    /// `reload_configuration`. A malformed or error response is dropped
    /// (nothing to reload from), not treated as fatal.
    fn handle_configuration_response(&mut self, resp: Response) -> Vec<Message> {
        let Ok(value) = resp.response_result else {
            return Vec::new();
        };
        let Ok(items) = serde_json::from_value::<Vec<serde_json::Value>>(value) else {
            return Vec::new();
        };
        match settings::parse_pull_response(&items) {
            Some(settings) => self.reload_configuration(settings),
            None => Vec::new(),
        }
    }

    /// The actual `workspace/didChangeConfiguration` work, shared by both
    /// the direct-push and pull-response paths: re-resolves each
    /// folder's config (honoring `settings.config_path`, resolved against
    /// the folder when relative) and republishes whatever changed.
    ///
    /// A true no-op - the freshly resolved config and `enable` flag both
    /// match what's already applied - returns `Vec::new()` without
    /// re-checking anything, since this notification fires for ANY client
    /// settings save under the registered section, not just a
    /// `phpstan-rs` one; re-checking every analyzed file on every
    /// unrelated settings.json save would be a real, common-case cost.
    /// A change that leaves the project's settings as they were (the
    /// level, `ignore_errors`, the baseline, `enable`) checks nothing
    /// either: the last findings are published again through the new
    /// post-filters.
    ///
    /// A change of the files the configuration loads (`paths`,
    /// `scan_paths`, `stub_paths`, `no_composer`) is applied as the
    /// difference between the file sets (`reload_file_set`).
    fn reload_configuration(&mut self, settings: settings::PhpstanRsSettings) -> Vec<Message> {
        let mut messages = self.finish_loads();
        if self.folders.is_empty() {
            return messages;
        }
        let was_enabled = self.enabled;
        let config_path_override = settings.config_path.as_deref().map(PathBuf::from);
        let inputs = ConfigInputs {
            path_override: config_path_override.as_deref(),
            schema: &self.config_schema,
            pool: &self.pool,
        };
        messages.extend(self.folders.iter_mut().flat_map(|workspace| {
            workspace.reload_configuration(settings.enable, was_enabled, &inputs)
        }));
        self.enabled = settings.enable;
        self.config_path_override = config_path_override;
        messages
    }
}

/// Correlates an inbound `Message::Response` with the outbound request
/// that caused it, via `Server::pending_requests`, then dispatches by
/// method. A response for an id this server never sent (or already
/// handled, e.g. a superseded earlier configuration pull) is logged and
/// dropped, not treated as an error.
fn handle_response<W: Write>(server: &mut Server, writer: &mut W, resp: Response) {
    let id = resp.id.clone();
    match server.pending_requests.outgoing.complete(id) {
        Some("workspace/configuration") => {
            for message in server.handle_configuration_response(resp) {
                let _ = write_message(writer, &message);
            }
        }
        Some("client/registerCapability") => {
            // A successful ack (`Result = ()`) needs no follow-up; an
            // error just gets logged - no client-facing error channel
            // exists, and file-watching simply stays inactive for this
            // session, same as a client that never advertised the
            // capability at all.
            if let Err(e) = resp.response_result {
                eprintln!("phpstan-rs lsp: client/registerCapability failed: {e:?}");
            }
        }
        Some(_method) => {}
        None => {
            eprintln!(
                "phpstan-rs lsp: response for an unknown or already-handled request id {:?}",
                resp.id
            );
        }
    }
}

/// No logic consumes the timeout tick beyond checking for a new message -
/// the debounce worker (VSCode plan item 2.2) owns its own independent
/// timer on a separate thread rather than piggybacking on this one, so a
/// slow debounced recheck can never delay this loop noticing a real
/// incoming request/notification. Exists so `run_loop` is never parked in
/// a blocking read - the literal capability this gives up by switching
/// from `read_message` (a blocking call on the reader itself) to
/// `recv_timeout` on a channel a background thread feeds instead.
const RUN_LOOP_TICK: Duration = Duration::from_millis(50);

/// `textDocument/didChange`'s own debounce window - coalesces a rapid
/// burst of keystrokes into one recheck instead of one per edit.
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(200);

/// The debounce worker's own wait cap when nothing is pending at all -
/// just frequent enough that a `shutdown` signal (which has no dedicated
/// wake of its own beyond `Condvar::notify_all`) is never noticed more
/// than this long after the main loop sets it, without busy-polling.
const DEBOUNCE_IDLE_WAIT: Duration = Duration::from_millis(200);

/// The shared, mutex-guarded coalescing queue `queue_change` (called
/// from the reader/main thread, via a `textDocument/didChange`
/// notification) and the debounce worker thread (which owns the timer
/// and drains it) both hold a reference to. Lock ordering is a single,
/// cycle-free chain: the one nesting is the worker's, always in the
/// order Server (held for the whole recheck) then this one (acquired-
/// and-released before each file's check, inside `should_abort`, by the
/// worker pool's threads the recheck waits on, and once more afterward
/// by the worker to confirm the generation still matches before
/// publishing). No thread holding this one waits for Server: the pool's
/// threads hold nothing else, and the reader thread only ever holds one
/// of the two at a time. No path exists for a deadlock.
struct DebounceHandle {
    queue: Mutex<debounce::DebounceQueue>,
    cv: Condvar,
}

/// Records `path`'s new edit and wakes the debounce worker - a handful
/// of instructions guarded by a briefly-held, uncontended lock, never
/// blocked by anything `Project`-related. This is the reader thread's
/// entire involvement in a `didChange` - no channel message is needed
/// for it, unlike every other request/notification.
fn queue_change(
    debounce: &Arc<DebounceHandle>,
    path: PathBuf,
    uri: Uri,
    text: String,
    version: i32,
) {
    debounce
        .queue
        .lock()
        .unwrap()
        .record_edit(path, uri, text, version, Instant::now());
    debounce.cv.notify_one();
}

/// Returns the process exit code `main.rs` should exit with (per the LSP
/// spec's `exit`-notification contract - see `run_loop`'s own doc
/// comment). `version` is the binary's, reported on `initialize`;
/// `config_schema` is every section a workspace config may set.
pub fn run(version: &str, config_schema: config::Schema) -> i32 {
    // `bounded`, not `unbounded`: backpressures a fast/misbehaving
    // client (blocks the reader thread, and so the OS pipe) instead of
    // buffering unboundedly.
    let (tx, rx) = crossbeam_channel::bounded(64);
    std::thread::Builder::new()
        .name("phpstan-rs-lsp-reader".to_string())
        .spawn(move || {
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            pump_messages(&mut reader, &tx);
        })
        .expect("spawning the LSP reader thread");
    // `io::Stdout` already does its own internal per-write locking - this
    // outer `Mutex` exists only so the debounce worker thread can share
    // the exact same `Write` handle `run_loop`'s own dispatch uses,
    // matching `writer`'s own new `Arc<Mutex<W>>` shape everywhere else.
    let writer = Arc::new(Mutex::new(io::stdout()));
    run_loop(&rx, writer, version, config_schema)
    // Not joined: once run_loop exits the process is ending anyway; the
    // reader thread, possibly still blocked in a real Message::read on
    // stdin with nothing left reading its sender, is simply abandoned -
    // same as today's single-threaded run() already does with any
    // buffered-but-unread trailing input. The debounce worker IS joined,
    // inside run_loop itself, before it returns.
}

/// The actual message loop, generic over its writer so tests can drive
/// it with an in-memory `Vec<u8>` instead of a real process's stdout.
/// Reads come off `receiver` instead of directly from an I/O reader -
/// `pump_messages` (on a background thread, in `run`'s real path, or
/// called synchronously first in a test) is what actually calls
/// `read_message`, so this loop can `recv_timeout` and never block
/// indefinitely inside a blocking read syscall.
///
/// `writer` is `Arc<Mutex<W>>`, taken by value, and `Server` itself is
/// now built and held behind its own `Arc<Mutex<_>>` internally - both
/// shared with the debounce worker thread this function spawns (VSCode
/// plan item 2.2), so a `didChange` burst can be coalesced and its
/// eventual recheck genuinely canceled mid-cascade by a newer edit
/// arriving while it's still running, rather than only ever observable
/// between two already-sequential `update_file` calls on one thread.
/// `handle_request`/`handle_notification`/`handle_response`'s own
/// signatures are unchanged (`&mut Server`/`&mut W`) - this loop (and the
/// worker) lock both, dispatch, and release, rather than propagating the
/// `Arc<Mutex<_>>` types into every call site and test that already
/// calls them directly.
///
/// Returns the process exit code the LSP spec's `exit` notification
/// mandates: 0 if a `shutdown` request preceded it, 1 otherwise. A
/// transport ending (clean EOF or a real I/O disconnection - both
/// collapse to the same `RecvTimeoutError::Disconnected` here, since
/// `pump_messages` already logs which one it actually was) is this
/// server's existing "no incident" end-of-session path and always
/// returns 0 regardless of `shutdown_received` - the spec's exit-code
/// contract binds the `exit` notification specifically, not every way a
/// session can end.
fn run_loop<W: Write + Send + 'static>(
    receiver: &Receiver<Message>,
    writer: Arc<Mutex<W>>,
    version: &str,
    config_schema: config::Schema,
) -> i32 {
    let server = Arc::new(Mutex::new(Server::new(version, config_schema)));
    let debounce = Arc::new(DebounceHandle {
        queue: Mutex::new(debounce::DebounceQueue::new(DEFAULT_DEBOUNCE)),
        cv: Condvar::new(),
    });
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker = spawn_debounce_worker(
        Arc::clone(&server),
        Arc::clone(&debounce),
        Arc::clone(&writer),
        Arc::clone(&shutdown),
    );

    let exit_code = run_dispatch_loop(receiver, &server, &writer, &debounce);

    // Shutdown handshake: wake the worker (a plain `store` alone would
    // never be noticed until its own next timeout/edit-driven wake),
    // then join it - a real drain point, not an assumption. The worker's
    // own shutdown branch flushes every still-pending edit
    // (`DebounceQueue::drain_all`) before returning, so this `join`
    // returns only once that final publish (if any) has gone through the
    // shared writer.
    shutdown.store(true, Ordering::Release);
    debounce.cv.notify_all();
    worker
        .join()
        .expect("joining the LSP debounce worker thread");

    exit_code
}

/// The main dispatch loop itself, split out of `run_loop` only so the
/// latter reads as "set up shared state, run the loop, tear down shared
/// state" - not a reusable entry point of its own.
fn run_dispatch_loop<W: Write>(
    receiver: &Receiver<Message>,
    server: &Arc<Mutex<Server>>,
    writer: &Arc<Mutex<W>>,
    debounce: &Arc<DebounceHandle>,
) -> i32 {
    let loaded_rx = server.lock().unwrap().loaded_rx.clone();
    loop {
        let message = crossbeam_channel::select! {
            recv(receiver) -> message => match message {
                Ok(message) => message,
                Err(_) => return 0,
            },
            recv(loaded_rx) -> loaded => {
                if let Ok(loaded) = loaded {
                    let messages = server.lock().unwrap().install_loaded(loaded);
                    let mut w = writer.lock().unwrap();
                    for message in messages {
                        let _ = write_message(&mut *w, &message);
                    }
                }
                continue;
            },
            default(RUN_LOOP_TICK) => continue,
        };

        match message {
            Message::Request(req) => handle_request(
                &mut server.lock().unwrap(),
                &mut *writer.lock().unwrap(),
                req,
            ),
            Message::Notification(not) => {
                if not.method == "exit" {
                    return if server.lock().unwrap().shutdown_received {
                        0
                    } else {
                        1
                    };
                }
                // `pump_messages` (the reader thread) forwards its own
                // recoverable-failure reports as a `window/logMessage`
                // notification on this same channel, since it has no
                // writer of its own - this loop is the only thing that
                // does, so it just writes it straight through rather
                // than routing it into `handle_notification`, which only
                // ever expects a real client-originated notification.
                if not.method == "window/logMessage" {
                    let _ =
                        write_message(&mut *writer.lock().unwrap(), &Message::Notification(not));
                    continue;
                }
                // `didChange` is diverted here, before `handle_notification`
                // ever sees it: queuing it is the reader/main thread's
                // entire involvement (see `queue_change`'s own doc
                // comment) - the debounce worker thread runs the actual
                // recheck once the edit's own debounce window elapses (or
                // is coalesced by a newer one first).
                if not.method == "textDocument/didChange" {
                    if let Ok(params) =
                        serde_json::from_value::<DidChangeTextDocumentParams>(not.params)
                        && let Some(path) = uri_to_path(&params.text_document.uri)
                        && let Some(change) = params.content_changes.into_iter().next()
                    {
                        queue_change(
                            debounce,
                            path,
                            params.text_document.uri,
                            change.text,
                            params.text_document.version,
                        );
                    }
                    continue;
                }
                // A save asks for the whole-program checks, which run once
                // the typing has stopped.
                if not.method == "textDocument/didSave" {
                    debounce
                        .queue
                        .lock()
                        .unwrap()
                        .request_whole_program(Instant::now());
                    debounce.cv.notify_one();
                    continue;
                }
                // A pending or in-flight debounced edit for a now-closed
                // file must not fire/publish - `cancel` drops both its
                // pending entry and its generation, so an in-flight
                // recheck's own `should_abort` (capturing a generation
                // `>= 1`) aborts the moment it next checks, since
                // `current_generation` for an absent path reads `0`.
                if not.method == "textDocument/didClose"
                    && let Ok(params) = serde_json::from_value::<
                        lsp_types::DidCloseTextDocumentParams,
                    >(not.params.clone())
                    && let Some(path) = uri_to_path(&params.text_document.uri)
                {
                    debounce.queue.lock().unwrap().cancel(&path);
                }
                handle_notification(
                    &mut server.lock().unwrap(),
                    &mut *writer.lock().unwrap(),
                    not,
                );
            }
            Message::Response(resp) => handle_response(
                &mut server.lock().unwrap(),
                &mut *writer.lock().unwrap(),
                resp,
            ),
        }
    }
}

/// Runs on its own dedicated thread, owning the debounce timer -
/// `queue_change` (the reader thread) only ever touches
/// `debounce.queue` briefly to record an edit and wake this thread via
/// `notify_one`; this is the only role that ever locks `Server` for a
/// recheck. A proper `Condvar` wait, not a busy-poll: it sleeps until
/// either the earliest pending deadline elapses or a new edit wakes it
/// early (which may push that deadline further out, or pull it closer
/// via a still-later-recorded generation - either way the loop just
/// recomputes and waits again).
fn spawn_debounce_worker<W: Write + Send + 'static>(
    server: Arc<Mutex<Server>>,
    debounce: Arc<DebounceHandle>,
    writer: Arc<Mutex<W>>,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("phpstan-rs-lsp-debounce".to_string())
        .spawn(move || {
            loop {
                let guard = debounce.queue.lock().unwrap();
                if shutdown.load(Ordering::Acquire) {
                    return flush_and_publish_all(&server, &debounce, &writer, guard);
                }
                let now = Instant::now();
                let wait = match guard.next_deadline() {
                    Some(deadline) => deadline.saturating_duration_since(now),
                    None => DEBOUNCE_IDLE_WAIT,
                };
                let (mut guard, _timed_out) = debounce.cv.wait_timeout(guard, wait).unwrap();
                if shutdown.load(Ordering::Acquire) {
                    return flush_and_publish_all(&server, &debounce, &writer, guard);
                }
                let expired = guard.drain_expired(Instant::now());
                drop(guard);
                if !expired.is_empty() {
                    run_pending_edits(&server, &debounce, &writer, expired);
                }
                let due = debounce
                    .queue
                    .lock()
                    .unwrap()
                    .take_whole_program(Instant::now());
                if due {
                    let messages = server.lock().unwrap().recheck_whole_program();
                    let mut w = writer.lock().unwrap();
                    for message in messages {
                        let _ = write_message(&mut *w, &message);
                    }
                }
            }
        })
        .expect("spawning the LSP debounce worker thread")
}

/// The shutdown path's own forced flush - ignores every remaining
/// deadline (a session ending mid-debounce-window must not silently drop
/// that edit) and runs every one of them before this function (and so
/// the worker thread, and so `run_loop`'s own `join`) returns.
fn flush_and_publish_all<W: Write>(
    server: &Arc<Mutex<Server>>,
    debounce: &Arc<DebounceHandle>,
    writer: &Arc<Mutex<W>>,
    mut guard: std::sync::MutexGuard<'_, debounce::DebounceQueue>,
) {
    let flushed = guard.drain_all();
    let whole_program = guard.take_whole_program_now();
    drop(guard);
    if !flushed.is_empty() {
        run_pending_edits(server, debounce, writer, flushed);
    }
    if whole_program {
        let messages = server.lock().unwrap().recheck_whole_program();
        let mut w = writer.lock().unwrap();
        for message in messages {
            let _ = write_message(&mut *w, &message);
        }
    }
}

/// Runs each drained edit's own recheck (holding `server`'s lock for the
/// whole cascade - see `DebounceHandle`'s own doc comment on the accepted
/// "hover/definition block behind a slow recheck" tradeoff), then
/// re-confirms against `debounce.queue` that this edit is STILL the
/// freshest one recorded for its path before publishing - covers the
/// small window between the cascade's own last per-file `should_abort`
/// check and its actual return. A `should_abort` closure captures the
/// edit's own generation and re-locks `debounce.queue` (never `server`)
/// before each file's check, on the worker pool's threads while this
/// thread holds `server` - the single point where the two locks nest,
/// always in the same Server-then-Debounce order.
fn run_pending_edits<W: Write>(
    server: &Arc<Mutex<Server>>,
    debounce: &Arc<DebounceHandle>,
    writer: &Arc<Mutex<W>>,
    edits: Vec<(PathBuf, debounce::PendingEdit)>,
) {
    for (path, pending) in edits {
        let my_generation = pending.generation;
        let abort_debounce = Arc::clone(debounce);
        let abort_path = path.clone();
        let should_abort = move || {
            abort_debounce
                .queue
                .lock()
                .unwrap()
                .current_generation(&abort_path)
                != my_generation
        };
        let messages = server.lock().unwrap().handle_debounced_change(
            &path,
            &pending.uri,
            &pending.text,
            pending.version,
            should_abort,
        );
        let still_current =
            debounce.queue.lock().unwrap().current_generation(&path) == my_generation;
        if still_current {
            let mut w = writer.lock().unwrap();
            for message in messages {
                let _ = write_message(&mut *w, &message);
            }
        }
    }
}

/// A successful response carrying `result` as it is: `Response::new_ok`
/// would serialize a value that already is JSON, copying all of it.
fn ok_response(id: &RequestId, result: serde_json::Value) -> Message {
    Message::Response(Response {
        id: id.clone(),
        response_result: Ok(result),
    })
}

/// Registers `req.id` (with a fresh cancellation flag) before dispatch
/// and writes the resulting response through `respond_unless_canceled`,
/// so a `"$/cancelRequest"` arriving mid-dispatch (impossible today,
/// under the fully synchronous `run_loop` - see this module's own notes
/// on `pending_incoming_requests` - but the shape a future concurrent
/// handler would need) can suppress the real answer instead of racing it.
fn handle_request<W: Write>(server: &mut Server, writer: &mut W, req: Request) {
    let id = req.id.clone();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    server
        .pending_incoming_requests
        .register(id.clone(), Arc::clone(&cancel_flag));

    if !matches!(req.method.as_str(), "initialize" | "shutdown") {
        for message in server.finish_loads() {
            let _ = write_message(writer, &message);
        }
    }

    let response = match req.method.as_str() {
        "initialize" => {
            let params: Option<InitializeParams> = serde_json::from_value(req.params).ok();
            #[allow(deprecated)] // root_uri: no other way to reach it pre-workspace_folders
            let (
                roots,
                position_encoding,
                progress_token,
                supports_configuration_pull,
                supports_watched_files,
            ) = match params {
                Some(params) => {
                    // Every real workspace_folders entry, not just the
                    // first - falling back to the single deprecated
                    // root_uri only when workspace_folders is absent or
                    // empty (a client that supports it but opened with
                    // no folders at all).
                    let roots: Vec<PathBuf> =
                        match params.workspace_folders.filter(|f| !f.is_empty()) {
                            Some(folders) => {
                                folders.iter().filter_map(|f| uri_to_path(&f.uri)).collect()
                            }
                            None => params
                                .root_uri
                                .as_ref()
                                .and_then(uri_to_path)
                                .into_iter()
                                .collect(),
                        };
                    // A client that offers general.positionEncodings with
                    // "utf-8" gets it: columns are then bytes, which the
                    // line index counts. Otherwise the spec's UTF-16
                    // default applies, counted in code units.
                    let position_encoding = params
                        .capabilities
                        .general
                        .and_then(|g| g.position_encodings)
                        .is_some_and(|encodings| encodings.contains(&PositionEncodingKind::UTF8))
                        .then_some(PositionEncodingKind::UTF8);
                    // A real vscode-languageclient client both supports
                    // and relies on this: it always sends
                    // workspace/didChangeConfiguration with an
                    // empty/placeholder settings value, so this pull is
                    // the only way this server ever learns the client's
                    // actual phpstan-rs.* settings from such a client.
                    let supports_configuration_pull = params
                        .capabilities
                        .workspace
                        .as_ref()
                        .and_then(|w| w.configuration)
                        .unwrap_or(false);
                    // Dynamic registration is the only mechanism LSP
                    // offers for workspace/didChangeWatchedFiles -
                    // there is no static "serverCapabilities" field
                    // for it, so this must be sent as a
                    // client/registerCapability request once
                    // "initialized" arrives (see "initialized"'s own
                    // handling below), never advertised here.
                    let supports_watched_files = params
                        .capabilities
                        .workspace
                        .as_ref()
                        .and_then(|w| w.did_change_watched_files.as_ref())
                        .and_then(|d| d.dynamic_registration)
                        .unwrap_or(false);
                    let progress_token = params.work_done_progress_params.work_done_token;
                    (
                        roots,
                        position_encoding,
                        progress_token,
                        supports_configuration_pull,
                        supports_watched_files,
                    )
                }
                None => (Vec::new(), None, None, false, false),
            };
            server.set_unit(if position_encoding.is_some() {
                Unit::Utf8
            } else {
                Unit::Utf16
            });
            server.supports_configuration_pull = supports_configuration_pull;
            server.supports_watched_files = supports_watched_files;
            // The scan itself (config resolution + parsing) runs from
            // "initialized", not here - see PendingWorkspaceScan's own
            // doc comment.
            if !roots.is_empty() {
                server.pending_workspace_scan = Some(PendingWorkspaceScan {
                    roots,
                    progress_token,
                });
            }
            let result = InitializeResult {
                capabilities: ServerCapabilities {
                    position_encoding,
                    // Full sync: didChange always carries the whole new
                    // document text, not incremental ranges - simplest
                    // correct choice, and update_file already takes a
                    // full source string anyway. A save, without the text,
                    // asks for the whole-program checks.
                    text_document_sync: Some(TextDocumentSyncCapability::Options(
                        lsp_types::TextDocumentSyncOptions {
                            open_close: Some(true),
                            change: Some(TextDocumentSyncKind::FULL),
                            save: Some(lsp_types::TextDocumentSyncSaveOptions::Supported(true)),
                            ..Default::default()
                        },
                    )),
                    hover_provider: Some(HoverProviderCapability::Simple(true)),
                    definition_provider: Some(OneOf::Left(true)),
                    type_definition_provider: Some(
                        lsp_types::TypeDefinitionProviderCapability::Simple(true),
                    ),
                    document_symbol_provider: Some(OneOf::Left(true)),
                    code_action_provider: Some(CodeActionProviderCapability::Options(
                        CodeActionOptions {
                            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                            ..Default::default()
                        },
                    )),
                    workspace_symbol_provider: Some(OneOf::Left(true)),
                    completion_provider: Some(CompletionOptions {
                        trigger_characters: Some(vec![">".to_string(), ":".to_string()]),
                        ..Default::default()
                    }),
                    signature_help_provider: Some(SignatureHelpOptions {
                        trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                        ..Default::default()
                    }),
                    document_highlight_provider: Some(OneOf::Left(true)),
                    inlay_hint_provider: Some(OneOf::Left(true)),
                    references_provider: Some(OneOf::Left(true)),
                    rename_provider: Some(OneOf::Left(true)),
                    semantic_tokens_provider: Some(
                        lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(
                            lsp_types::SemanticTokensOptions {
                                legend: semantic_tokens::legend(),
                                range: None,
                                full: Some(lsp_types::SemanticTokensFullOptions::Bool(true)),
                                ..Default::default()
                            },
                        ),
                    ),
                    workspace: Some(lsp_types::WorkspaceServerCapabilities {
                        workspace_folders: Some(lsp_types::WorkspaceFoldersServerCapabilities {
                            supported: Some(true),
                            change_notifications: Some(OneOf::Left(true)),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                server_info: Some(ServerInfo {
                    name: "phpstan-rs".to_string(),
                    version: Some(server.version.clone()),
                }),
            };
            Message::Response(Response::new_ok(id.clone(), result))
        }
        "shutdown" => {
            server.shutdown_received = true;
            ok_response(&id, serde_json::Value::Null)
        }
        "textDocument/hover" => {
            let result = serde_json::from_value::<HoverParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position_params.text_document.uri,
                        |workspace| {
                            workspace.hover(
                                &params.text_document_position_params.text_document.uri,
                                params.text_document_position_params.position,
                            )
                        },
                    )
                })
                .map(|hover| serde_json::to_value(hover).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/definition" => {
            let result = serde_json::from_value::<GotoDefinitionParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position_params.text_document.uri,
                        |workspace| {
                            workspace.definition(
                                &params.text_document_position_params.text_document.uri,
                                params.text_document_position_params.position,
                            )
                        },
                    )
                })
                .map(|loc| serde_json::to_value(loc).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/typeDefinition" => {
            let result = serde_json::from_value::<GotoDefinitionParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position_params.text_document.uri,
                        |workspace| {
                            workspace.type_definition(
                                &params.text_document_position_params.text_document.uri,
                                params.text_document_position_params.position,
                            )
                        },
                    )
                })
                .map(|loc| serde_json::to_value(loc).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/documentSymbol" => {
            let result = serde_json::from_value::<DocumentSymbolParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(&params.text_document.uri, |workspace| {
                        workspace.document_symbol(&params.text_document.uri)
                    })
                })
                .map(|symbols| serde_json::to_value(symbols).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/codeAction" => {
            let result = serde_json::from_value::<CodeActionParams>(req.params)
                .ok()
                .and_then(|params| {
                    // A client-requested kind `R` matches a server-
                    // produced kind `K` when `K == R` or `K` specializes
                    // `R` (`K` starts with `R + "."`) - never the other
                    // way around. This server only ever emits the bare
                    // `quickfix` kind (no `.`-suffix), so it can only
                    // ever satisfy a request for exactly `quickfix`; a
                    // request for a narrower kind like `quickfix.remove`
                    // has nothing here to offer and short-circuits.
                    let offers_quickfix = match &params.context.only {
                        Some(only) => only.contains(&CodeActionKind::QUICKFIX),
                        None => true,
                    };
                    if !offers_quickfix {
                        return Some(CodeActionResponse::new());
                    }
                    server.on_document(&params.text_document.uri, |workspace| {
                        workspace.code_actions(&params.text_document.uri, params.range)
                    })
                })
                .map(|actions| serde_json::to_value(actions).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "workspace/symbol" => {
            let result = serde_json::from_value::<WorkspaceSymbolParams>(req.params)
                .ok()
                .map(|params| server.workspace_symbol(&params.query))
                .map(|symbols| {
                    serde_json::to_value(WorkspaceSymbolResponse::Flat(symbols))
                        .unwrap_or(serde_json::Value::Null)
                })
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/completion" => {
            let result = serde_json::from_value::<CompletionParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position.text_document.uri,
                        |workspace| {
                            workspace.completion(
                                &params.text_document_position.text_document.uri,
                                params.text_document_position.position,
                            )
                        },
                    )
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/signatureHelp" => {
            let result = serde_json::from_value::<SignatureHelpParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position_params.text_document.uri,
                        |workspace| {
                            workspace.signature_help(
                                &params.text_document_position_params.text_document.uri,
                                params.text_document_position_params.position,
                            )
                        },
                    )
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/documentHighlight" => {
            let result = serde_json::from_value::<DocumentHighlightParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position_params.text_document.uri,
                        |workspace| {
                            workspace.document_highlight(
                                &params.text_document_position_params.text_document.uri,
                                params.text_document_position_params.position,
                            )
                        },
                    )
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/semanticTokens/full" => {
            let result = serde_json::from_value::<lsp_types::SemanticTokensParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(&params.text_document.uri, |workspace| {
                        workspace.semantic_tokens_full(&params.text_document.uri)
                    })
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/inlayHint" => {
            let result = serde_json::from_value::<InlayHintParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(&params.text_document.uri, |workspace| {
                        workspace.inlay_hints(&params.text_document.uri, params.range)
                    })
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/references" => {
            let result = serde_json::from_value::<lsp_types::ReferenceParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position.text_document.uri,
                        |workspace| {
                            workspace.references(
                                &params.text_document_position.text_document.uri,
                                params.text_document_position.position,
                                params.context.include_declaration,
                            )
                        },
                    )
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        // What the session holds: each loaded file with its role, and the
        // registry dump (`project::registry_dump`), for the session record
        // (`session_record`).
        "phpstan-rs/sessionDump" => {
            let workspace = server.folders.first().unwrap_or(&server.unrooted);
            let files: Vec<serde_json::Value> = workspace
                .project
                .file_roles()
                .into_iter()
                .map(|(path, analysed)| {
                    serde_json::json!([
                        if analysed { "analysed" } else { "context" },
                        path.display().to_string()
                    ])
                })
                .collect();
            ok_response(
                &id,
                serde_json::json!({
                    "files": files,
                    "registry": workspace.project.registry_dump(),
                }),
            )
        }
        "textDocument/rename" => {
            let result = serde_json::from_value::<lsp_types::RenameParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.on_document(
                        &params.text_document_position.text_document.uri,
                        |workspace| {
                            workspace.rename(
                                &params.text_document_position.text_document.uri,
                                params.text_document_position.position,
                                &params.new_name,
                            )
                        },
                    )
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        other => Message::Response(Response::new_err(
            id.clone(),
            ErrorCode::MethodNotFound as i32,
            format!("method not found: {other}"),
        )),
    };
    respond_unless_canceled(server, writer, &id, response);
}

/// Writes `response` only if `id` hasn't already been answered by a
/// `"$/cancelRequest"`'s own `RequestCanceled` error - `Incoming::complete`
/// both looks up and removes the pending entry in one step, so this and a
/// cancellation can never both write a response for the same id.
fn respond_unless_canceled<W: Write>(
    server: &mut Server,
    writer: &mut W,
    id: &RequestId,
    response: Message,
) {
    if server.pending_incoming_requests.complete(id).is_some() {
        let _ = write_message(writer, &response);
    }
}

fn handle_notification<W: Write>(server: &mut Server, writer: &mut W, not: LspNotification) {
    match not.method.as_str() {
        "initialized" => {
            if let Some(scan) = server.pending_workspace_scan.take() {
                for message in server.start_scan(scan) {
                    let _ = write_message(writer, &message);
                }
            }
            if let Some(request) = server.request_watched_files_registration() {
                let _ = write_message(writer, &request);
            }
        }
        "$/cancelRequest" => {
            if let Ok(params) = serde_json::from_value::<CancelParams>(not.params) {
                let id = to_request_id(params.id);
                // `Incoming::cancel(id)` discards the registered flag
                // (its own body is `let _data = self.complete(&id)?;`),
                // so it can't be used here - `complete` directly, then
                // set the flag and hand-build the identical error
                // response ourselves.
                if let Some(flag) = server.pending_incoming_requests.complete(&id) {
                    flag.store(true, Ordering::Relaxed);
                    let _ = write_message(
                        writer,
                        &Message::Response(Response::new_err(
                            id,
                            ErrorCode::RequestCanceled as i32,
                            "canceled by client".to_string(),
                        )),
                    );
                }
                // No pending entry: already answered, or a stray cancel
                // for an id this server never saw - both are normal,
                // silent no-ops per the LSP spec.
            }
        }
        "workspace/didChangeWorkspaceFolders" => {
            if let Ok(params) =
                serde_json::from_value::<lsp_types::DidChangeWorkspaceFoldersParams>(not.params)
            {
                // Removed before added: a folder simultaneously removed
                // and re-added in the same event (some clients model a
                // rename this way) should drop-then-reload, not race a
                // transient "already tracked" against itself.
                let removed: Vec<PathBuf> = params
                    .event
                    .removed
                    .iter()
                    .filter_map(|f| uri_to_path(&f.uri))
                    .collect();
                if !removed.is_empty() {
                    for message in server.remove_workspace_folders(&removed) {
                        let _ = write_message(writer, &message);
                    }
                }
                let added: Vec<PathBuf> = params
                    .event
                    .added
                    .iter()
                    .filter_map(|f| uri_to_path(&f.uri))
                    .collect();
                if !added.is_empty() {
                    server.start_loading(&added);
                }
            }
        }
        "workspace/didChangeConfiguration" => {
            if let Ok(params) =
                serde_json::from_value::<lsp_types::DidChangeConfigurationParams>(not.params)
            {
                if let Some(settings) = settings::extract_pushed_settings(&params.settings) {
                    for message in server.reload_configuration(settings) {
                        let _ = write_message(writer, &message);
                    }
                } else if let Some(request) = server.request_configuration_pull() {
                    let _ = write_message(writer, &request);
                } else {
                    // No pushed value and no pull support: still worth
                    // re-resolving from disk (picks up an on-disk
                    // phpstan-rs.toml edit), just with no way to learn of
                    // a changed phpstan-rs.enable/configPath client
                    // setting from this client. Reconstructed from the
                    // currently-applied values, not defaults, so this
                    // fallback can't itself reset an override a prior
                    // reload already applied.
                    let settings = settings::PhpstanRsSettings {
                        enable: server.enabled,
                        config_path: server
                            .config_path_override
                            .as_ref()
                            .map(|p| p.display().to_string()),
                    };
                    for message in server.reload_configuration(settings) {
                        let _ = write_message(writer, &message);
                    }
                }
            }
        }
        "workspace/didChangeWatchedFiles" => {
            if let Ok(params) =
                serde_json::from_value::<lsp_types::DidChangeWatchedFilesParams>(not.params)
            {
                for message in server.handle_watched_files_changed(params.changes) {
                    let _ = write_message(writer, &message);
                }
            }
        }
        "textDocument/didOpen" => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(not.params) {
                let version = params.text_document.version;
                let uri = params.text_document.uri;
                for message in
                    server.handle_open_or_change(&uri, &params.text_document.text, Some(version))
                {
                    let _ = write_message(writer, &message);
                }
            }
        }
        // `textDocument/didChange` is deliberately NOT handled here -
        // `run_dispatch_loop` diverts it to `queue_change` before
        // `handle_notification` is ever called at all (VSCode plan item
        // 2.2's own debounce/coalesce mechanism). A direct test call to
        // `handle_notification` with a didChange notification is
        // therefore a no-op, matching this arm's own absence rather than
        // silently double-applying the edit through two different paths.
        "textDocument/didClose" => {
            // The file stays in the project (its registry entries and
            // dependency edges are still real and other open files may
            // depend on them), it's just no longer an open document: the
            // disk's file is what the session reads of it from now on.
            if let Ok(params) =
                serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(not.params.clone())
                && let Some(path) = uri_to_path(&params.text_document.uri)
            {
                server.close_document(&path);
            }
        }
        _ => {
            // An unrecognized notification is just ignored, per the spec.
        }
    }
}

#[cfg(test)]
mod tests;
