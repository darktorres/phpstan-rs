//! A minimal Language Server Protocol server, run via `phpstan-rs --lsp`.
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
//! `checker::Diagnostic` carries a real `Span`, converted here to LSP's
//! 0-indexed line/character convention. The `location` symbolic
//! label (e.g. `"ClassName::methodName"`) is still folded into the
//! message text rather than dropped - LSP has no field for it. Known
//! remaining imprecision: LSP wants UTF-16 code-unit columns, and this
//! project's `Span` counts UTF-8 characters - an acceptable approximation
//! for now (both agree for any line without astral-plane characters, the
//! overwhelming majority of real PHP source), not a correctness bug worth
//! blocking on.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use lsp_server::{ErrorCode, Message, Notification as LspNotification, Request, Response};
use lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, Location,
    MarkedString, MessageType, OneOf, Position as LspPosition, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};

use crate::ast::ClassKind;
use crate::checker::Checker;
use crate::project::Project;
use crate::registry::format_signature;
use crate::token::Span;
use crate::types::Type;

mod document_symbol;
mod navigation;
mod protocol;
mod text_split;

#[cfg(test)]
use protocol::percent_decode;
use protocol::{
    from_lsp_position, log_message_notification, path_to_uri, publish_diagnostics_notification,
    pump_messages, to_lsp_range, uri_to_path, write_message,
};

/// Every `Object` class name reachable from a `Checker::probe_hit`
/// result, in order - a bare `Object`, or (a receiver built from more
/// than one code path) every `Object` member of a `Union`/`Intersection`,
/// not just the first - `resolve_member_owners` below decides which of
/// these candidates actually has the member being navigated to. Empty
/// for anything else (`Mixed`, a scalar, ...) - an un-inferrable
/// receiver resolves to nothing, never a guess.
fn type_to_classes(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Object(class, _) => out.push(class.clone()),
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                type_to_classes(m, out);
            }
        }
        _ => {}
    }
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
    let mut candidates = Vec::new();
    type_to_classes(ty, &mut candidates);
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

struct Server {
    project: Project,
    // Every URI the client currently has open or has otherwise seen, so
    // `initialize`'s workspace scan and `didOpen`/`didChange` share one
    // path->diagnostics story instead of the workspace scan being
    // invisible to later incremental updates.
    open_uris: HashMap<PathBuf, Uri>,
}

impl Server {
    fn new() -> Self {
        let mut project = Project::new();
        // hover/definition's own text-split technique needs each file's
        // exact source text - see `Project::retain_source`'s own doc
        // comment for why this is opt-in and off elsewhere.
        project.set_retain_source(true);
        Server {
            project,
            open_uris: HashMap::new(),
        }
    }

    /// Loads every `.php` file under `root` into the project up front, so
    /// the cross-file registry and dependency graph this whole phase is
    /// about are populated from the very first diagnostic, not built up
    /// one `didOpen` at a time.
    fn load_workspace(&mut self, root: &Path) -> Vec<Message> {
        let mut files = Vec::new();
        crate::project::collect_php_files(root, &mut files);
        // See main.rs's run_check_project for why this matters: a real
        // workspace can genuinely declare the same fully-qualified class
        // twice (duplicate vendored copies of a library), and
        // unsorted readdir order would make which one wins the
        // registry slot filesystem-dependent.
        files.sort();
        let mut messages = Vec::new();
        for path in files {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let source = String::from_utf8_lossy(&bytes).into_owned();
            // Momentarily-invalid syntax is the normal state while a
            // file is being edited elsewhere in the workspace, not an
            // edge case - a single malformed file here must not prevent
            // the rest of the workspace from loading at all, matching
            // main.rs's own try_load_file for the equivalent check-
            // project path.
            let path_for_log = path.clone();
            if let Err(e) = self.project.load_file(path, &source) {
                eprintln!(
                    "phpstan-rs lsp: skipping {} - failed to parse: {e}",
                    path_for_log.display()
                );
                messages.push(log_message_notification(
                    MessageType::WARNING,
                    format!(
                        "phpstan-rs: skipping {} - failed to parse: {e}",
                        path_for_log.display()
                    ),
                ));
            }
        }
        self.project.finish_loading();
        messages
    }

    fn handle_open_or_change(&mut self, uri: &Uri, text: &str) -> Vec<Message> {
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        self.open_uris.insert(path.clone(), uri.clone());
        // Same graceful-degradation as load_workspace, and more exposed
        // in practice here: momentarily-invalid syntax is the normal
        // state while a file is being typed, not an edge case.
        // `update_file`'s `Result` makes a parse failure here a plain,
        // recoverable `Err` rather than unwinding and taking down the
        // whole LSP server process.
        let results = match self.project.update_file(&path, text) {
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
        results
            .into_iter()
            .map(|(p, diags)| {
                // A dependent file re-checked here might not itself be
                // open in the editor - it still needs a fresh
                // publishDiagnostics, using whatever URI it was last seen
                // under (the workspace scan, if nothing else).
                let dep_uri = self
                    .open_uris
                    .get(&p)
                    .cloned()
                    .unwrap_or_else(|| path_to_uri(&p));
                publish_diagnostics_notification(dep_uri, &diags)
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
        let path = uri_to_path(uri)?;
        let resolved = navigation::resolve_at(&self.project, &path, from_lsp_position(position))?;
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
                let kind = match info.kind {
                    ClassKind::Class => "class",
                    ClassKind::Interface => "interface",
                    ClassKind::Trait => "trait",
                    ClassKind::Enum => "enum",
                };
                let mut text = format!("{kind} {fqn}");
                if !info.extends.is_empty() {
                    text.push_str(&format!(" extends {}", info.extends.join(", ")));
                }
                if !info.implements.is_empty() {
                    text.push_str(&format!(" implements {}", info.implements.join(", ")));
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
            }
            | navigation::Resolved::VariableRead {
                span,
                scope,
                is_static,
                class,
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
            return Some(format!("{owner}::${name}: {}", sig.ty));
        }
        if let Some((_, sig)) = registry.lookup_const_with_owner(owner, name) {
            return Some(format!("{owner}::{name}: {}", sig.ty));
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
        let mut checker = Checker::new(self.project.registry());
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
        let mut checker = Checker::new(self.project.registry());
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

    /// The `navigation::Resolved` symbol's own declaration site, via
    /// `Project::decl_locations` - `None` when nothing resolved, or a
    /// symbol resolved by name but declared nowhere this project has
    /// loaded (an undefined reference - already flagged by an ordinary
    /// diagnostic, not this method's job to report again).
    fn definition(&self, uri: &Uri, position: LspPosition) -> Option<GotoDefinitionResponse> {
        let path = uri_to_path(uri)?;
        let resolved = navigation::resolve_at(&self.project, &path, from_lsp_position(position))?;
        // A local variable's own declaration site(s) are same-file and
        // never need a `decl_locations` lookup at all, unlike every other
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
        let locations = self.project.decl_locations();
        match &resolved {
            navigation::Resolved::Class(fqn) => {
                let (target_path, span) = locations.classes.get(fqn)?;
                Some(GotoDefinitionResponse::Scalar(
                    self.location(target_path, *span),
                ))
            }
            navigation::Resolved::Function(fqn) => {
                let (target_path, span) = locations.functions.get(fqn)?;
                Some(GotoDefinitionResponse::Scalar(
                    self.location(target_path, *span),
                ))
            }
            navigation::Resolved::Member(owner, name) => {
                let (target_path, span) = locations.members.get(&(owner.clone(), name.clone()))?;
                Some(GotoDefinitionResponse::Scalar(
                    self.location(target_path, *span),
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
                let owners = resolve_member_owners(self.project.registry(), &ty, member);
                let mut locs: Vec<Location> = Vec::new();
                for owner in &owners {
                    if let Some((target_path, span)) =
                        locations.members.get(&(owner.clone(), member.clone()))
                    {
                        locs.push(self.location(target_path, *span));
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

    fn document_symbol(&self, uri: &Uri) -> Option<DocumentSymbolResponse> {
        let path = uri_to_path(uri)?;
        let stmts = self.project.ast(&path)?;
        Some(DocumentSymbolResponse::Nested(
            document_symbol::document_symbols(stmts),
        ))
    }

    fn location(&self, target_path: &Path, span: Span) -> Location {
        let target_uri = self
            .open_uris
            .get(target_path)
            .cloned()
            .unwrap_or_else(|| path_to_uri(target_path));
        Location {
            uri: target_uri,
            range: to_lsp_range(span),
        }
    }
}

/// No logic consumes the timeout tick yet - a future debounce mechanism
/// flushes any pending recheck here. Exists so `run_loop` is never
/// parked in a blocking read - the literal capability this gives up by
/// switching from `read_message` (a blocking call on the reader itself)
/// to `recv_timeout` on a channel a background thread feeds instead.
const RUN_LOOP_TICK: Duration = Duration::from_millis(50);

pub fn run() {
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
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    run_loop(&rx, &mut writer);
    // Not joined: once run_loop exits the process is ending anyway; the
    // reader thread, possibly still blocked in a real Message::read on
    // stdin with nothing left reading its sender, is simply abandoned -
    // same as today's single-threaded run() already does with any
    // buffered-but-unread trailing input.
}

/// The actual message loop, generic over its writer so tests can drive
/// it with an in-memory `Vec<u8>` instead of a real process's stdout.
/// Reads come off `receiver` instead of directly from an I/O reader -
/// `pump_messages` (on a background thread, in `run`'s real path, or
/// called synchronously first in a test) is what actually calls
/// `read_message`, so this loop can `recv_timeout` and never block
/// indefinitely inside a blocking read syscall.
fn run_loop<W: Write>(receiver: &Receiver<Message>, writer: &mut W) {
    let mut server = Server::new();

    loop {
        let message = match receiver.recv_timeout(RUN_LOOP_TICK) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };

        match message {
            Message::Request(req) => handle_request(&mut server, writer, req),
            Message::Notification(not) => {
                if not.method == "exit" {
                    break;
                }
                // `pump_messages` (the reader thread) forwards its own
                // recoverable-failure reports as a `window/logMessage`
                // notification on this same channel, since it has no
                // writer of its own - this loop is the only thing that
                // does, so it just writes it straight through rather
                // than routing it into `handle_notification`, which only
                // ever expects a real client-originated notification.
                if not.method == "window/logMessage" {
                    let _ = write_message(writer, &Message::Notification(not));
                    continue;
                }
                handle_notification(&mut server, writer, not);
            }
            // This server never sends a request of its own, so it never
            // expects a response back from the client either.
            Message::Response(_) => {}
        }
    }
}

fn handle_request<W: Write>(server: &mut Server, writer: &mut W, req: Request) {
    match req.method.as_str() {
        "initialize" => {
            #[allow(deprecated)] // root_uri: no other way to reach it pre-workspace_folders
            let root = serde_json::from_value::<InitializeParams>(req.params)
                .ok()
                .and_then(|params| {
                    params
                        .root_uri
                        .or_else(|| params.workspace_folders?.into_iter().next().map(|f| f.uri))
                });
            let mut startup_messages = Vec::new();
            if let Some(root) = root.as_ref().and_then(uri_to_path) {
                startup_messages = server.load_workspace(&root);
            }
            let result = InitializeResult {
                capabilities: ServerCapabilities {
                    // Full sync: didChange always carries the whole new
                    // document text, not incremental ranges - simplest
                    // correct choice, and update_file already takes a
                    // full source string anyway.
                    text_document_sync: Some(TextDocumentSyncCapability::Kind(
                        TextDocumentSyncKind::FULL,
                    )),
                    hover_provider: Some(HoverProviderCapability::Simple(true)),
                    definition_provider: Some(OneOf::Left(true)),
                    document_symbol_provider: Some(OneOf::Left(true)),
                    ..Default::default()
                },
                server_info: Some(ServerInfo {
                    name: "phpstan-rs".to_string(),
                    version: Some(env!("CARGO_PKG_VERSION").to_string()),
                }),
            };
            let _ = write_message(writer, &Message::Response(Response::new_ok(req.id, result)));
            for message in startup_messages {
                let _ = write_message(writer, &message);
            }
        }
        "shutdown" => {
            let _ = write_message(
                writer,
                &Message::Response(Response::new_ok(req.id, serde_json::Value::Null)),
            );
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
            let _ = write_message(writer, &Message::Response(Response::new_ok(req.id, result)));
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
            let _ = write_message(writer, &Message::Response(Response::new_ok(req.id, result)));
        }
        "textDocument/documentSymbol" => {
            let result = serde_json::from_value::<DocumentSymbolParams>(req.params)
                .ok()
                .and_then(|params| server.document_symbol(&params.text_document.uri))
                .map(|symbols| serde_json::to_value(symbols).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            let _ = write_message(writer, &Message::Response(Response::new_ok(req.id, result)));
        }
        other => {
            let _ = write_message(
                writer,
                &Message::Response(Response::new_err(
                    req.id,
                    ErrorCode::MethodNotFound as i32,
                    format!("method not found: {other}"),
                )),
            );
        }
    }
}

fn handle_notification<W: Write>(server: &mut Server, writer: &mut W, not: LspNotification) {
    match not.method.as_str() {
        "initialized" => {} // nothing to do yet
        "textDocument/didOpen" => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(not.params) {
                let uri = params.text_document.uri;
                for message in server.handle_open_or_change(&uri, &params.text_document.text) {
                    let _ = write_message(writer, &message);
                }
            }
        }
        "textDocument/didChange" => {
            if let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(not.params) {
                let uri = params.text_document.uri;
                // Full sync (see the `textDocumentSync: FULL` capability
                // above) guarantees exactly one entry whose `text` is the
                // complete new document.
                if let Some(change) = params.content_changes.into_iter().next() {
                    for message in server.handle_open_or_change(&uri, &change.text) {
                        let _ = write_message(writer, &message);
                    }
                }
            }
        }
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
