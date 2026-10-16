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

use config::ConfigFile;
use crossbeam_channel::{Receiver, RecvTimeoutError};
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
    Range as LspRange, SaveOptions, ServerCapabilities, ServerInfo, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, SignatureInformation, SymbolInformation,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Uri, WorkDoneProgress, WorkDoneProgressBegin, WorkDoneProgressEnd,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use crate::app::check_project::{self, CheckRequest};
use crate::checker::Checker;
use crate::project::session::{
    Classified, DEFAULT_REPORT_LEVEL, DiagnosticOrder, ErrorPolicy, Finalize, Session,
    classify_unlisted, finalize_results,
};
use crate::project::source::{self, FileSource};
use crate::project::use_sites;
use crate::project::walk::{self, Listed, Role};
use crate::project::{Project, ProjectSettings};
use crate::registry::stub_set::{StubSource, Stubs};
use crate::registry::{FunctionReflection, format_signature, param_label};
use crate::{baseline, ignore_errors};
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
mod settings;
mod signature_help;
mod workspace_symbol;

#[cfg(test)]
use protocol::percent_decode;
use protocol::{
    Positions, diagnostic_fix_to_code_action, log_message_notification, offset_from_lsp,
    path_to_uri, progress_notification, publish_diagnostics_notification, pump_messages,
    range_to_lsp, ranges_overlap, show_message_notification, to_request_id, uri_to_path,
    write_message,
};

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
/// for every other variant - `Server::same_symbol`'s own quick pre-
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

struct Server {
    project: Project,
    // The built-in stubs, which the project takes its base from.
    stubs: Stubs,
    // Every URI the client currently has open or has otherwise seen, so
    // the (now-deferred) workspace scan and `didOpen`/`didChange` share
    // one path->diagnostics story instead of the workspace scan being
    // invisible to later incremental updates. Also carries the last
    // known LSP document version for that path (`None` if only ever seen
    // via the workspace scan or a `workspace/didChangeWatchedFiles`
    // event - neither carries a document version, only `didOpen`/
    // `didChange` do), so `publishDiagnostics` can be stamped with it -
    // see `uri_and_version_for`.
    open_uris: FxHashMap<PathBuf, (Uri, Option<i32>)>,
    // The last-computed, post-`finalize_for_publish` diagnostic set for
    // every path this server has ever checked - `code_actions` reads
    // this instead of re-running a whole-file checker pass per request
    // (a real editor's lightbulb fires `textDocument/codeAction` far
    // more often than actual edits, e.g. on cursor movement). Holds what
    // the editor was last sent for each path, and only a publish changes
    // it: a set that was not sent is never stored here, and
    // `publish_analysis` compares against it to decide what to send. A
    // removed file's entry goes with its clearing notification
    // (`publish_removal`). Safe under
    // today's fully synchronous, one-message-at-a-time `run_loop`: a
    // `codeAction` request can only ever see whatever the most recently
    // processed `didChange`/scan already committed, never a torn or
    // stale-relative-to-an-earlier-unprocessed-edit state.
    last_diagnostics: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    // The raw per-file checks of every analysed path: the last full
    // analysis's, updated by the files each edit re-checked. These are what
    // `finalize_for_publish` post-processes before anything is published.
    file_results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    // The raw project-wide diagnostics of the last full analysis, by the
    // file each is reported in. An edit drops the edited file's own entry,
    // whose spans do not fit the edited text, until the next save.
    whole_results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    // The session the project was configured from: a full analysis runs its
    // checks with this session's configuration.
    session: Option<Session>,
    // The exact current text `didOpen`/`didChange` last sent for a path,
    // regardless of whether it parses - `Project::source`/`Project::ast`
    // only ever reflect the last *successfully parsed* version, but
    // `textDocument/completion`'s member-completion path needs the LIVE
    // (very often mid-edit and unparseable - `$x->` alone is a syntax
    // error) buffer text to tokenize and to extract a receiver
    // expression's own text from. See `completion`'s own module doc
    // comment for the full mechanism this feeds.
    latest_text: FxHashMap<PathBuf, String>,
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
    // Every workspace folder root already loaded, as the client named it and
    // as it resolves - `add_workspace_folders`/`remove_workspace_folders`
    // dedup/filter against the first, and the project keys the files a folder
    // loaded by the second, which a folder whose link was since removed no
    // longer resolves to.
    workspace_roots: Vec<(PathBuf, PathBuf)>,
    // The most recently configured workspace root, which `finalize_for_publish`
    // rewrites a diagnostic's path against before consulting the applied
    // `ignore_error_entries` and `baseline`. With more than one workspace
    // folder, this and `applied` are last-root-wins: `Project`/`Registry`
    // have no per-root config scoping.
    root: Option<PathBuf>,
    // What `root`'s configuration applies, once it has been applied.
    applied: Option<Applied>,
    // A `phpstan-rs.configPath` setting override, applied on top of
    // `root`'s own default `phpstan-rs.toml` lookup by every subsequent
    // `add_workspace_folders`/`reload_configuration` call. `None`
    // until a `workspace/didChangeConfiguration` notification sets one.
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
    // The version `initialize` reports, the binary's own.
    version: String,
    // Every section a workspace's `phpstan-rs.toml` may set.
    config_schema: config::Schema,
}

/// What a workspace's configuration applies to the server: the settings its
/// project runs with, the post-filters its diagnostics pass through, and the
/// files it lists. A reload compares the newly resolved one with the applied
/// one by real `PartialEq`, never a `Debug` fingerprint: the SQL reflection
/// config's `Debug` redacts its password.
#[derive(PartialEq)]
struct Applied {
    settings: ProjectSettings,
    baseline: Option<baseline::Baseline>,
    ignore_error_entries: Vec<ignore_errors::IgnoreErrorEntry>,
    listing: Vec<Listed>,
}

/// One `window/showMessage` warning for each problem a configuration has.
fn configuration_warnings(warnings: Vec<String>) -> Vec<Message> {
    warnings
        .into_iter()
        .map(|warning| {
            show_message_notification(MessageType::WARNING, format!("phpstan-rs: {warning}"))
        })
        .collect()
}

impl Server {
    fn new(version: &str, config_schema: config::Schema) -> Self {
        let stubs = Stubs::new(
            StubSource::from_environment(),
            crate::project::worker_pool(),
        );
        let mut project = Project::from_stubs(&stubs);
        // hover/definition's own text-split technique needs each file's
        // exact source text - see `Project::retain_source`'s own doc
        // comment for why this is opt-in and off elsewhere.
        project.apply_settings(ProjectSettings {
            retain_source: true,
            ..ProjectSettings::default()
        });
        Server {
            project,
            stubs,
            open_uris: FxHashMap::default(),
            last_diagnostics: FxHashMap::default(),
            file_results: FxHashMap::default(),
            whole_results: FxHashMap::default(),
            session: None,
            latest_text: FxHashMap::default(),
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
            workspace_roots: Vec::new(),
            root: None,
            applied: None,
            config_path_override: None,
            enabled: true,
            supports_configuration_pull: false,
            supports_watched_files: false,
            file_versions: FxHashMap::default(),
            inlay_hint_cache: FxHashMap::default(),
            version: version.to_string(),
            config_schema,
        }
    }

    /// Resolves `root`'s configuration into what the server applies: the
    /// `config_path` given, or else `root`'s own `phpstan-rs.toml`. It
    /// follows `check-project`'s rules under `ErrorPolicy::Degrade`, so a
    /// problem the configuration cannot be used with comes back as a
    /// warning, and the part it concerns takes its default. A missing file
    /// is an empty configuration, not a problem.
    fn resolve_workspace(
        &self,
        root: &Path,
        config_path: Option<&Path>,
    ) -> Result<(Applied, Session, Vec<String>), String> {
        let mut warnings = Vec::new();
        let config_path =
            config_path.map_or_else(|| root.join("phpstan-rs.toml"), Path::to_path_buf);
        let config = if config_path.is_file() {
            ConfigFile::load(&config_path, &self.config_schema)
                .and_then(|config| config.validate(&self.config_schema).map(|()| config))
                .unwrap_or_else(|error| {
                    warnings.push(error);
                    ConfigFile::empty()
                })
        } else {
            ConfigFile::empty()
        };
        let run = check_project::prepare(
            &CheckRequest::default(),
            &config,
            self.stubs.clone(),
            root.to_path_buf(),
            ErrorPolicy::Degrade,
        )?;
        warnings.extend(run.notices());
        let baseline = match run.load_baseline() {
            Some(Ok(baseline)) => Some(baseline),
            Some(Err(line)) => {
                warnings.push(line);
                None
            }
            None => None,
        };
        let applied = Applied {
            settings: run.session.editor_settings(),
            baseline,
            ignore_error_entries: run.session.ignore_error_entries().to_vec(),
            listing: run.session.listing(),
        };
        Ok((applied, run.session, warnings))
    }

    /// Applies `next` to `self.project` and `self`, then loads the files it
    /// lists that are not loaded yet, context first. `previous` is the listing
    /// the project was configured with before: a file it listed under another
    /// role, or not at all, is removed first, unless the editor has it open.
    /// Returns the messages of those removals and loads, and whether a file
    /// was loaded, in which case the caller runs `finish_loading_with` once
    /// for its batch.
    ///
    /// **Context-before-root load order is a correctness invariant, not
    /// just a dedup convenience**: `Registry::insert_built_decl`'s
    /// handling of a second unconditional same-name declaration is
    /// itself order-sensitive - context-then-analyzed hits the
    /// `analyzed: true` branch (an outright overwrite, discarding the
    /// context entry's members), while analyzed-then-context would
    /// instead hit the `analyzed: false` branch (`existing.merge(...)`,
    /// folding the vendor info into the first-party entry) - a
    /// genuinely different `Project` state for the identical tree. A
    /// future refactor of this method must preserve this order
    /// explicitly.
    fn apply_configuration(
        &mut self,
        next: Applied,
        session: Session,
        previous: &[Listed],
    ) -> (Vec<Message>, bool) {
        let mut messages = Vec::new();
        self.project.apply_settings(next.settings.clone());

        let current: FxHashMap<&Path, (Role, bool)> = next
            .listing
            .iter()
            .map(|listed| (listed.path.as_path(), (listed.role, listed.stub)))
            .collect();
        let removed: Vec<PathBuf> = {
            let loaded: FxHashSet<&Path> = self.project.loaded_paths().collect();
            previous
                .iter()
                .filter(|old| current.get(old.path.as_path()) != Some(&(old.role, old.stub)))
                .filter(|old| {
                    loaded.contains(old.path.as_path()) && !self.open_uris.contains_key(&old.path)
                })
                .map(|old| old.path.clone())
                .collect()
        };
        // Published once the new configuration is in place, so the dependents'
        // diagnostics are finalized with it.
        let removal = if removed.is_empty() {
            None
        } else {
            for path in &removed {
                self.latest_text.remove(path);
            }
            Some(self.project.remove_files(&removed))
        };

        let to_load: Vec<Listed> = {
            let loaded: FxHashSet<&Path> = self.project.loaded_paths().collect();
            next.listing
                .iter()
                .filter(|listed| !loaded.contains(listed.path.as_path()))
                .cloned()
                .collect()
        };
        let loaded = !to_load.is_empty();
        for (path, reason) in session.load_listed(&mut self.project, &to_load) {
            eprintln!(
                "phpstan-rs lsp: skipping {} - failed to parse: {reason}",
                path.display()
            );
            messages.push(log_message_notification(
                MessageType::WARNING,
                format!(
                    "phpstan-rs: skipping {} - failed to parse: {reason}",
                    path.display()
                ),
            ));
        }
        self.applied = Some(next);
        self.session = Some(session);
        if let Some(results) = removal {
            messages.extend(self.publish_removal(&removed, results));
        }
        (messages, loaded)
    }

    /// Loads every not-yet-tracked root in `roots` (deduped against
    /// `self.workspace_roots`), then runs `Project::finish_loading_with` at
    /// most ONCE for the whole batch - not once per root, which would
    /// pay N whole-registry recomputes for N roots (each re-scanning
    /// every previously-loaded root's own files again) instead of one.
    /// Called from both `"initialized"` (the `pending_workspace_scan`
    /// roots resolved at `"initialize"`) and
    /// `"workspace/didChangeWorkspaceFolders"`'s own `added` array -
    /// every load path goes through this one method, even for a single
    /// root, so there is exactly one place this batching discipline can
    /// be violated.
    fn add_workspace_folders(&mut self, roots: &[PathBuf]) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut any_loaded = false;
        for root in roots {
            if self
                .workspace_roots
                .iter()
                .any(|(spelled, _)| spelled == root)
            {
                continue;
            }
            // What a root's configuration applies to `self`/`self.project` is
            // last-root-wins: `Project`/`Registry` have no per-root config
            // scoping, only one shared instance for however many folders are
            // open.
            match self.resolve_workspace(root, self.config_path_override.as_deref()) {
                Ok((next, session, warnings)) => {
                    messages.extend(configuration_warnings(warnings));
                    self.root = Some(root.clone());
                    let (applied, loaded) = self.apply_configuration(next, session, &[]);
                    messages.extend(applied);
                    any_loaded |= loaded;
                    self.workspace_roots
                        .push((root.clone(), walk::resolve(root)));
                }
                Err(error) => messages.push(show_message_notification(
                    MessageType::ERROR,
                    format!("phpstan-rs: {error}"),
                )),
            }
        }
        if any_loaded {
            self.project.finish_loading_with(&self.stubs);
        }
        messages
    }

    /// Batch counterpart to `add_workspace_folders`: drops every loaded
    /// path under any of `roots` (`Project::remove_files`, one recompute
    /// for the whole batch) and clears `open_uris`/publishes an empty
    /// `publishDiagnostics` for any of them that was open in the editor -
    /// otherwise a document whose backing folder just vanished would
    /// keep showing stale squiggles indefinitely, since removing a
    /// workspace folder is not itself a `textDocument/didClose`.
    fn remove_workspace_folders(&mut self, roots: &[PathBuf]) -> Vec<Message> {
        let resolved: Vec<PathBuf> = roots
            .iter()
            .map(|root| {
                self.workspace_roots
                    .iter()
                    .find(|(spelled, _)| spelled == root)
                    .map_or_else(|| walk::resolve(root), |(_, resolved)| resolved.clone())
            })
            .collect();
        self.workspace_roots
            .retain(|(spelled, _)| !roots.contains(spelled));
        let paths: Vec<PathBuf> = self
            .project
            .loaded_paths()
            .filter(|p| resolved.iter().any(|root| p.starts_with(root)))
            .map(Path::to_path_buf)
            .collect();

        let results = self.project.remove_files(&paths);
        let messages = self.publish_removal(&paths, results);
        for p in &paths {
            self.open_uris.remove(p);
            self.latest_text.remove(p);
        }
        messages
    }

    /// Runs the full analysis of the loaded project - every analysed file's
    /// checks, then the project-wide passes, as `check-project` runs them -
    /// and keeps its raw results for publishing. A file whose check panicked
    /// keeps no results.
    fn analyse_workspace(&mut self) {
        let Some(session) = &self.session else {
            self.file_results.clear();
            self.whole_results.clear();
            return;
        };
        let mut paths = self.project.analyzed_paths();
        // Sorted, so repeated runs against an unchanged workspace check the
        // same files in the same order.
        paths.sort();
        let checks = session.check_project(&mut self.project, &paths, false);
        for (path, msg) in &checks.check_panics {
            eprintln!(
                "phpstan-rs lsp: checking {} panicked: {msg}",
                path.display()
            );
        }
        self.file_results = checks.per_file;
        self.whole_results = checks.whole_program;
    }

    /// The raw diagnostics of `paths`: each path's per-file checks, then its
    /// project-wide diagnostics.
    fn raw_results_for(
        &self,
        paths: &[PathBuf],
    ) -> FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>> {
        paths
            .iter()
            .map(|path| {
                let mut diags = self.file_results.get(path).cloned().unwrap_or_default();
                if let Some(whole) = self.whole_results.get(path) {
                    diags.extend(whole.iter().cloned());
                }
                (path.clone(), diags)
            })
            .collect()
    }

    /// The post-processing a published diagnostic set gets: the one
    /// `check-project` applies, except unmatched ignore comments, which the
    /// server never reports.
    fn finalize_for_publish(
        &self,
        results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    ) -> FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>> {
        let options = match (&self.session, &self.applied) {
            (Some(session), Some(applied)) => Finalize {
                report_unmatched_ignores: false,
                baseline: applied.baseline.clone(),
                ..session.finalize_options()
            },
            _ => Finalize {
                level: DEFAULT_REPORT_LEVEL,
                report_unmatched_ignores: false,
                ignore_errors: Vec::new(),
                baseline: None,
                order: DiagnosticOrder::Sorted,
            },
        };
        finalize_results(
            &self.project,
            results,
            &options,
            self.project.absolute_root(),
        )
        .results
    }

    /// Publishes what the last analysis changed: each analysed path whose
    /// finalized diagnostics differ from what the editor was last sent. A
    /// path with nothing to report is published only when it had something
    /// before, so the editor sees it cleared.
    fn publish_analysis(&mut self) -> Vec<Message> {
        let mut paths = self.project.analyzed_paths();
        paths.sort();
        let mut finalized = self.finalize_for_publish(self.raw_results_for(&paths));
        let previous = std::mem::take(&mut self.last_diagnostics);
        let mut all: Vec<PathBuf> = previous.keys().chain(finalized.keys()).cloned().collect();
        all.sort();
        all.dedup();
        let mut messages = Vec::new();
        for path in all {
            let diags = finalized.remove(&path).unwrap_or_default();
            let changed = match previous.get(&path) {
                Some(before) => *before != diags,
                None => !diags.is_empty(),
            };
            if changed {
                let (uri, version) = self.uri_and_version_for(&path);
                messages.push(self.diagnostics_message(&path, uri, version, &diags));
            }
            self.last_diagnostics.insert(path, diags);
        }
        messages
    }

    /// The messages of an edit whose per-file checks are `results`: the
    /// edited file's and its dependents'. The edited file's project-wide
    /// diagnostics point at text that has since changed, so they are dropped
    /// until the next save.
    fn publish_edit(
        &mut self,
        edited: &Path,
        results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    ) -> Vec<Message> {
        let finalized = self.finalize_edit(edited, results);
        self.publish_diagnostics_for(finalized)
    }

    /// Records an edit's per-file checks and returns the finalized diagnostics
    /// of the edited file and its dependents, without sending them. A debounced
    /// run that ends up superseded still records the results it computed; the
    /// caller decides whether they are sent.
    fn finalize_edit(
        &mut self,
        edited: &Path,
        results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    ) -> FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>> {
        self.whole_results.remove(edited);
        let mut paths = Vec::with_capacity(results.len() + 1);
        for (path, diags) in results {
            self.file_results.insert(path.clone(), diags);
            paths.push(path);
        }
        if !paths.iter().any(|path| path.as_path() == edited) {
            paths.push(edited.to_path_buf());
        }
        self.finalize_for_publish(self.raw_results_for(&paths))
    }

    /// The messages of a removal of `removed` whose re-checked dependents
    /// are `results`. Every removed file gets an empty set, which clears
    /// whatever the editor shows for it.
    fn publish_removal(
        &mut self,
        removed: &[PathBuf],
        mut results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        for path in removed {
            self.file_results.remove(path);
            self.whole_results.remove(path);
            self.last_diagnostics.remove(path);
            let (uri, version) = self.uri_and_version_for(path);
            messages.push(self.diagnostics_message(path, uri, version, &[]));
        }
        // The removed paths were cleared above, so they are not published again.
        results.retain(|path, _| !removed.contains(path));
        results.retain(|path, _| !removed.contains(path));
        let mut paths = Vec::with_capacity(results.len());
        for (path, diags) in results {
            self.file_results.insert(path.clone(), diags);
            paths.push(path);
        }
        let finalized = self.finalize_for_publish(self.raw_results_for(&paths));
        messages.extend(self.publish_diagnostics_for(finalized));
        messages
    }

    /// A save's own work: the full analysis, published where it changed.
    fn analyse_on_save(&mut self) -> Vec<Message> {
        if !self.enabled {
            return Vec::new();
        }
        self.analyse_workspace();
        self.publish_analysis()
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

    /// The key `path` has in the project: the path resolved through links,
    /// by the session when one is configured.
    fn identify(&self, path: &Path) -> PathBuf {
        match &self.session {
            Some(session) => session.identify(path),
            None => walk::resolve(path),
        }
    }

    /// The file `path` names, as the project keys it, with the role and stub
    /// status the configuration gives it.
    fn classify(&self, path: &Path) -> Classified {
        match &self.session {
            Some(session) => session.classify(path),
            None => classify_unlisted(path),
        }
    }

    /// The key of the file the document `uri` names, or `None` when the URI
    /// names no file.
    fn document_path(&self, uri: &Uri) -> Option<PathBuf> {
        Some(self.identify(&uri_to_path(uri)?))
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
        self.analyse_workspace();
        self.publish_analysis()
    }

    /// `reload_configuration`'s own republish step, once it decides
    /// something actually changed. Publishes each file whose diagnostics
    /// differ from what the editor was last sent, so a file that had
    /// diagnostics under the old config and none under the new one gets an
    /// explicit empty `publishDiagnostics` that clears them client-side.
    /// While `!self.enabled`, skips rechecking entirely and just clears every
    /// currently-open file - phpstan-rs.enable means "stop showing squiggles",
    /// not "stop the language server".
    fn republish_known_files(&mut self) -> Vec<Message> {
        if !self.enabled {
            // The editor now shows nothing for an open file, which the next
            // publish has to change from.
            for path in self.open_uris.keys() {
                self.last_diagnostics.insert(path.clone(), Vec::new());
            }
            let unit = self.unit;
            return self
                .open_uris
                .values()
                .cloned()
                .map(|(uri, version)| {
                    publish_diagnostics_notification(uri, version, &[], None, unit)
                })
                .collect();
        }
        self.analyse_workspace();
        self.publish_analysis()
    }

    fn handle_open_or_change(
        &mut self,
        uri: &Uri,
        text: &str,
        version: Option<i32>,
    ) -> Vec<Message> {
        let Some(spelled) = uri_to_path(uri) else {
            return Vec::new();
        };
        let Classified {
            path,
            role,
            is_stub,
        } = self.classify(&spelled);
        *self.file_versions.entry(path.clone()).or_insert(0) += 1;
        self.open_uris.insert(path.clone(), (uri.clone(), version));
        self.latest_text.insert(path.clone(), text.to_string());
        self.project.set_document(&path, text.as_bytes());
        // Opening a file exactly as it was loaded changes nothing in the
        // project, so only its own diagnostics need publishing.
        if self.project.content_hash(&path) == Some(source::content_hash(text.as_bytes())) {
            let diags = match self.last_diagnostics.get(&path) {
                Some(diags) => diags.clone(),
                None => {
                    let checked =
                        FxHashMap::from_iter([(path.clone(), self.project.check_file(&path))]);
                    self.finalize_for_publish(checked)
                        .remove(&path)
                        .unwrap_or_default()
                }
            };
            return self.publish_diagnostics_for(FxHashMap::from_iter([(path, diags)]));
        }
        // Same graceful degradation as a file that fails to parse on load,
        // and more exposed
        // in practice here: momentarily-invalid syntax is the normal
        // state while a file is being typed, not an edge case.
        // `update_file_as`'s `Result` makes a parse failure here a plain,
        // recoverable `Err` rather than unwinding and taking down the
        // whole LSP server process.
        let results =
            match self
                .project
                .update_file_as(&path, text, role == Role::Analysed, is_stub, || false)
            {
                Ok(results) => results,
                Err(e) => {
                    eprintln!(
                        "phpstan-rs lsp: skipping {} - failed to parse: {e}",
                        path.display()
                    );
                    return vec![log_message_notification(
                        MessageType::WARNING,
                        format!(
                            "phpstan-rs: skipping {} - failed to parse: {e}",
                            path.display()
                        ),
                    )];
                }
            };
        self.publish_edit(&path, results)
    }

    /// The debounce worker's own counterpart to `handle_open_or_change`
    /// (VSCode plan item 2.2) - identical except it drives `Project::
    /// update_file_cancellable` with a real `should_abort` closure
    /// instead of `update_file`, and takes an already-resolved `path`
    /// (the worker already has it as the `DebounceQueue` entry's own
    /// key) rather than re-deriving it from `uri`. `version` is a plain
    /// `i32`, not `Option<i32>` - a debounced edit always came from a
    /// real `didChange`, which always carries one. A run that `should_abort`
    /// reports superseded returns no messages: the newer edit's run sends the
    /// file's diagnostics instead.
    fn handle_debounced_change(
        &mut self,
        path: &Path,
        uri: &Uri,
        text: &str,
        version: i32,
        should_abort: &mut impl FnMut() -> bool,
    ) -> Vec<Message> {
        let classified = self.classify(path);
        let path = classified.path.as_path();
        let analysed = classified.role == Role::Analysed;
        *self.file_versions.entry(path.to_path_buf()).or_insert(0) += 1;
        self.open_uris
            .insert(path.to_path_buf(), (uri.clone(), Some(version)));
        self.latest_text
            .insert(path.to_path_buf(), text.to_string());
        self.project.set_document(path, text.as_bytes());
        let results = match self.project.update_file_as(
            path,
            text,
            analysed,
            classified.is_stub,
            &mut *should_abort,
        ) {
            Ok(results) => results,
            Err(e) => {
                eprintln!(
                    "phpstan-rs lsp: skipping {} - failed to parse: {e}",
                    path.display()
                );
                return vec![log_message_notification(
                    MessageType::WARNING,
                    format!(
                        "phpstan-rs: skipping {} - failed to parse: {e}",
                        path.display()
                    ),
                )];
            }
        };
        let finalized = self.finalize_edit(path, results);
        if should_abort() {
            return Vec::new();
        }
        // Files an earlier cancelled check left unchecked keep the diagnostics
        // the editor was last sent. Their raw results are stale, so they
        // are dropped until a save's full analysis recomputes them.
        for stale in self.project.take_unchecked() {
            if !finalized.contains_key(&stale) {
                self.file_results.remove(&stale);
                self.whole_results.remove(&stale);
            }
        }
        self.publish_diagnostics_for(finalized)
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
    fn handle_watched_files_changed(&mut self, changes: Vec<lsp_types::FileEvent>) -> Vec<Message> {
        let mut last_by_path: FxHashMap<PathBuf, (Uri, lsp_types::FileChangeType)> =
            FxHashMap::default();
        for change in changes {
            let Some(path) = uri_to_path(&change.uri) else {
                continue;
            };
            last_by_path.insert(self.identify(&path), (change.uri, change.typ));
        }

        let mut messages = Vec::new();
        for (path, (uri, typ)) in last_by_path {
            if typ == lsp_types::FileChangeType::DELETED {
                self.latest_text.remove(&path);
                self.project.unset_document(&path);
                let results = self.project.remove_file(&path);
                messages.extend(self.publish_removal(std::slice::from_ref(&path), results));
                self.open_uris.remove(&path);
                continue;
            }
            // Created and Changed both just need the path's current
            // on-disk content reflected - `update_file` already tolerates
            // a path with no prior entry (a brand-new file), so there is
            // no separate load-vs-update branch to make here. A read
            // failure (raced with a subsequent delete before this ran, a
            // transient permission error, ...) is skipped silently: a
            // later Deleted event, if any, cleans it up properly.
            let Ok(bytes) = FileSource::read_disk(&path) else {
                continue;
            };
            let Classified { role, is_stub, .. } = self.classify(&path);
            // No document version exists for an externally-observed
            // change (there is no didOpen/didChange behind it) - `None`
            // matches a file that was only ever loaded from disk.
            self.open_uris.insert(path.clone(), (uri, None));
            self.latest_text
                .insert(path.clone(), String::from_utf8_lossy(&bytes).into_owned());
            self.project.unset_document(&path);
            match self.project.update_file_as(
                &path,
                &bytes,
                role == Role::Analysed,
                is_stub,
                || false,
            ) {
                Ok(results) => {
                    messages.extend(self.publish_edit(&path, results));
                }
                Err(e) => {
                    eprintln!(
                        "phpstan-rs lsp: skipping {} - failed to parse: {e}",
                        path.display()
                    );
                }
            }
        }
        messages
    }

    /// Converts a diagnostics-by-path map into `publishDiagnostics`
    /// messages, resolving each path to whatever URI the client last
    /// used for it (a re-checked dependent might not itself be open in
    /// the editor - it still needs a fresh notification, using the
    /// workspace scan's own synthesized URI if nothing else ever opened
    /// it). Shared by `handle_open_or_change` and
    /// `remove_workspace_folders`'s own recheck step.
    fn publish_diagnostics_for(
        &mut self,
        results: FxHashMap<PathBuf, Vec<diagnostics::Diagnostic>>,
    ) -> Vec<Message> {
        results
            .into_iter()
            .map(|(p, diags)| {
                let (dep_uri, dep_version) = self.uri_and_version_for(&p);
                let message = self.diagnostics_message(&p, dep_uri, dep_version, &diags);
                self.last_diagnostics.insert(p, diags);
                message
            })
            .collect()
    }

    /// `navigation::resolve_at` finds what symbol is under the cursor;
    /// this formats what the registry already knows about it into hover
    /// text. `None` for anything unresolved (a local variable's own
    /// read/declaration included - a real position, just not one hover
    /// has anything useful to say about) - a valid "no hover here" per
    /// the LSP spec, not an error.
    fn hover(&self, uri: &Uri, position: LspPosition) -> Option<Hover> {
        let path = self.document_path(uri)?;
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
                let info = registry.classes.get(fqn.as_str())?;
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
        if let Some((_, sig)) = registry.lookup_const_with_owner(owner, name) {
            return Some(format!("{owner}::{name}: {}", sig.ty()));
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
        let path = self.document_path(uri)?;
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
        let path = self.document_path(uri)?;
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
        let path = self.document_path(uri)?;
        let stmts = self.project.ast(&path)?;
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
        let path = self.document_path(uri)?;
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
                vec![use_sites::RefTarget::Member(owner.clone(), name.clone())]
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
                    .map(|owner| use_sites::RefTarget::Member(owner, member.clone()))
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
        let path = self.document_path(uri)?;
        let resolved =
            navigation::resolve_at(&self.project, &path, self.offset_at(&path, position)?)?;
        let targets = self.ref_targets_for(&resolved);
        if targets.is_empty() {
            return None;
        }
        self.project.ensure_member_index_built();
        let mut locations: Vec<Location> = Vec::new();
        for target in &targets {
            for (p, span) in self.project.use_sites().references(target) {
                locations.push(self.location(p, *span));
            }
        }
        if include_declaration {
            let registry = self.project.registry();
            for target in &targets {
                let decl = match target {
                    use_sites::RefTarget::Class(fqn) => registry.class_declaration(fqn),
                    use_sites::RefTarget::Function(fqn) => registry.function_declaration(fqn),
                    use_sites::RefTarget::Member(owner, name) => {
                        registry.member_declaration(owner, name)
                    }
                };
                if let Some((p, span)) = decl {
                    locations.push(self.location(p, span));
                }
            }
        }
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
        let path = self.document_path(uri)?;
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
        let mut changes: FxHashMap<Uri, Vec<lsp_types::TextEdit>> = FxHashMap::default();
        for (p, span) in self.project.use_sites().references(&target) {
            let (edit_uri, _) = self.uri_and_version_for(p);
            changes
                .entry(edit_uri)
                .or_default()
                .push(lsp_types::TextEdit {
                    range: self.lsp_range(p, *span),
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
        let path = self.document_path(uri)?;
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
        let path = self.document_path(uri)?;
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
        let path = self.document_path(uri)?;
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
        let path = self.document_path(uri)?;
        let text = self
            .latest_text
            .get(&path)
            .cloned()
            .or_else(|| self.project.source(&path).map(str::to_string))?;
        let stmts = self.project.ast(&path)?;
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
    /// carry a `DiagnosticFix`. Reads `self.last_diagnostics` rather than
    /// re-running the checker - see that field's own doc comment for why.
    /// A cache miss (a workspace file requested before any `didOpen`/
    /// `didChange`/scan ever populated it) is computed via
    /// `Project::check_file` and finalized the same way every published set
    /// is, but not cached: a miss means nothing was sent for the file, and
    /// the cache holds only what was sent.
    fn code_actions(&mut self, uri: &Uri, range: LspRange) -> Option<CodeActionResponse> {
        let path = self.document_path(uri)?;
        let computed;
        let diagnostics = match self.last_diagnostics.get(&path) {
            Some(diags) => diags,
            None => {
                let raw = self.project.check_file(&path);
                computed = self
                    .finalize_for_publish(FxHashMap::from_iter([(path.clone(), raw)]))
                    .into_values()
                    .next()
                    .unwrap_or_default();
                &computed
            }
        };
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
    /// support at `"initialize"`. Scoped to `**/*.php` only, not also
    /// `composer.{json,lock}`: LSP mode has no composer-aware reload path
    /// to route those through yet, so watching them today would just
    /// misroute them into `update_file` as invalid PHP source for no
    /// benefit.
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
                                    watchers: vec![lsp_types::FileSystemWatcher {
                                        glob_pattern: lsp_types::GlobPattern::String(
                                            "**/*.php".to_string(),
                                        ),
                                        kind: None,
                                    }],
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
    /// the direct-push and pull-response paths: re-resolves `self.root`'s
    /// config (honoring `settings.config_path`, resolved against `root`
    /// when relative) and applies what changed.
    ///
    /// A true no-op - the freshly resolved configuration and `enable` flag
    /// both match what's already applied - returns `Vec::new()` without
    /// re-checking anything, since this notification fires for ANY client
    /// settings save under the registered section, not just a
    /// `phpstan-rs` one; re-checking every analyzed file on every
    /// unrelated settings.json save would be a real, common-case cost.
    ///
    /// Otherwise the file set changes by its difference from what was
    /// applied (`apply_configuration`), and the warnings about the
    /// configuration are reported.
    fn reload_configuration(&mut self, settings: settings::PhpstanRsSettings) -> Vec<Message> {
        let Some(root) = self.root.clone() else {
            return Vec::new();
        };
        let was_enabled = self.enabled;
        let config_path_override = settings.config_path.as_deref().map(|p| {
            let candidate = PathBuf::from(p);
            if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            }
        });
        let resolved = self.resolve_workspace(&root, config_path_override.as_deref());
        self.enabled = settings.enable;
        self.config_path_override = config_path_override;
        let (next, session, warnings) = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                return vec![show_message_notification(
                    MessageType::ERROR,
                    format!("phpstan-rs: {error}"),
                )];
            }
        };
        if settings.enable == was_enabled && self.applied.as_ref() == Some(&next) {
            return Vec::new();
        }

        let mut messages = configuration_warnings(warnings);
        let previous = self.applied.take();
        let previous: &[Listed] = previous.as_ref().map_or(&[], |old| old.listing.as_slice());
        let (applied, loaded) = self.apply_configuration(next, session, previous);
        messages.extend(applied);
        if loaded {
            self.project.finish_loading_with(&self.stubs);
        }
        messages.extend(self.republish_known_files());
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
/// cycle-free chain: the worker is the only role that ever holds both
/// this and `Server`'s own lock at once, always in the order Server
/// (held for the whole recheck) then this one (acquired-and-released
/// once per file, inside `should_abort`, and once more afterward to
/// confirm the generation still matches before publishing); the reader
/// thread only ever holds one of the two at a time, never nested. No
/// path exists for a deadlock.
struct DebounceHandle {
    queue: Mutex<debounce::DebounceQueue>,
    cv: Condvar,
}

/// The key the project has for the file `uri` names, for the reader thread,
/// which holds no session and must not wait for the server's lock. It is
/// the path `Session::identify` gives a file that exists.
fn document_key(uri: &Uri) -> Option<PathBuf> {
    Some(walk::resolve(&uri_to_path(uri)?))
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
    loop {
        let message = match receiver.recv_timeout(RUN_LOOP_TICK) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return 0,
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
                        && let Some(path) = document_key(&params.text_document.uri)
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
                // A save queues the project's full analysis for the debounce
                // worker, which runs it once no further save has arrived within
                // the debounce window, after any edits due by then.
                if not.method == "textDocument/didSave" {
                    debounce.queue.lock().unwrap().record_save(Instant::now());
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
                    && let Some(path) = document_key(&params.text_document.uri)
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
                let now = Instant::now();
                let expired = guard.drain_expired(now);
                let save_due = guard.take_due_save(now);
                drop(guard);
                if !expired.is_empty() {
                    run_pending_edits(&server, &debounce, &writer, expired);
                }
                if save_due {
                    run_pending_save(&server, &writer);
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
    drop(guard);
    if !flushed.is_empty() {
        run_pending_edits(server, debounce, writer, flushed);
    }
}

/// Runs each drained edit's own recheck (holding `server`'s lock for the
/// whole cascade - see `DebounceHandle`'s own doc comment on the accepted
/// "hover/definition block behind a slow recheck" tradeoff), then writes
/// what it sends. A `should_abort` closure captures the edit's own
/// generation and re-locks `debounce.queue` (never `server`) on each
/// per-file check - the single point where this worker thread nests both
/// locks, always in the same Server-then-Debounce order. Whether to send is
/// decided by `handle_debounced_change` after the cascade, under the same
/// server lock, so a superseded run never changes what the editor was last
/// sent.
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
        let mut should_abort = move || {
            abort_debounce
                .queue
                .lock()
                .unwrap()
                .current_generation(&abort_path)
                != my_generation
        };
        // The write happens while `server` is still held: a save's publish
        // can then never be overtaken by this older one.
        let mut server = server.lock().unwrap();
        let messages = server.handle_debounced_change(
            &path,
            &pending.uri,
            &pending.text,
            pending.version,
            &mut should_abort,
        );
        let mut w = writer.lock().unwrap();
        for message in messages {
            let _ = write_message(&mut *w, &message);
        }
    }
}

/// Runs a save's full analysis on the debounce worker and writes what it
/// changed, holding `server` across the write as `run_pending_edits` does.
fn run_pending_save<W: Write>(server: &Arc<Mutex<Server>>, writer: &Arc<Mutex<W>>) {
    let mut server = server.lock().unwrap();
    let messages = server.analyse_on_save();
    let mut w = writer.lock().unwrap();
    for message in messages {
        let _ = write_message(&mut *w, &message);
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
            server.unit = if position_encoding.is_some() {
                Unit::Utf8
            } else {
                Unit::Utf16
            };
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
                    // full source string anyway.
                    // Saves carry no text: the project already holds the saved file's
                    // latest text from the edits that preceded the save.
                    text_document_sync: Some(TextDocumentSyncCapability::Options(
                        TextDocumentSyncOptions {
                            open_close: Some(true),
                            change: Some(TextDocumentSyncKind::FULL),
                            save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                                include_text: Some(false),
                            })),
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
                    server.hover(
                        &params.text_document_position_params.text_document.uri,
                        params.text_document_position_params.position,
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
                    server.definition(
                        &params.text_document_position_params.text_document.uri,
                        params.text_document_position_params.position,
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
                    server.type_definition(
                        &params.text_document_position_params.text_document.uri,
                        params.text_document_position_params.position,
                    )
                })
                .map(|loc| serde_json::to_value(loc).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/documentSymbol" => {
            let result = serde_json::from_value::<DocumentSymbolParams>(req.params)
                .ok()
                .and_then(|params| server.document_symbol(&params.text_document.uri))
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
                    server.code_actions(&params.text_document.uri, params.range)
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
                    server.completion(
                        &params.text_document_position.text_document.uri,
                        params.text_document_position.position,
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
                    server.signature_help(
                        &params.text_document_position_params.text_document.uri,
                        params.text_document_position_params.position,
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
                    server.document_highlight(
                        &params.text_document_position_params.text_document.uri,
                        params.text_document_position_params.position,
                    )
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/semanticTokens/full" => {
            let result = serde_json::from_value::<lsp_types::SemanticTokensParams>(req.params)
                .ok()
                .and_then(|params| server.semantic_tokens_full(&params.text_document.uri))
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/inlayHint" => {
            let result = serde_json::from_value::<InlayHintParams>(req.params)
                .ok()
                .and_then(|params| server.inlay_hints(&params.text_document.uri, params.range))
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/references" => {
            let result = serde_json::from_value::<lsp_types::ReferenceParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.references(
                        &params.text_document_position.text_document.uri,
                        params.text_document_position.position,
                        params.context.include_declaration,
                    )
                })
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            ok_response(&id, result)
        }
        "textDocument/rename" => {
            let result = serde_json::from_value::<lsp_types::RenameParams>(req.params)
                .ok()
                .and_then(|params| {
                    server.rename(
                        &params.text_document_position.text_document.uri,
                        params.text_document_position.position,
                        &params.new_name,
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
                if let Some(token) = &scan.progress_token {
                    let _ = write_message(
                        writer,
                        &progress_notification(
                            token.clone(),
                            WorkDoneProgress::Begin(WorkDoneProgressBegin {
                                title: "phpstan-rs: scanning workspace".to_string(),
                                cancellable: Some(false),
                                message: None,
                                percentage: None,
                            }),
                        ),
                    );
                }
                for message in server.add_workspace_folders(&scan.roots) {
                    let _ = write_message(writer, &message);
                }
                for message in server.publish_workspace_diagnostics() {
                    let _ = write_message(writer, &message);
                }
                if let Some(token) = scan.progress_token {
                    let _ = write_message(
                        writer,
                        &progress_notification(
                            token,
                            WorkDoneProgress::End(WorkDoneProgressEnd { message: None }),
                        ),
                    );
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
                    for message in server.add_workspace_folders(&added) {
                        let _ = write_message(writer, &message);
                    }
                    for message in server.publish_workspace_diagnostics() {
                        let _ = write_message(writer, &message);
                    }
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
            // Deliberately a no-op: the file stays in the project (its
            // registry entries and dependency edges are still real and
            // other open files may depend on them), it's just no longer
            // tracked as "currently open" for the purposes of picking a
            // URI to publish under later.
        }
        _ => {
            // An unrecognized notification is just ignored, per the spec.
        }
    }
}

#[cfg(test)]
mod tests;
