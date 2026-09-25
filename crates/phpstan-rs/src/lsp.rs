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
//! `diagnostic::Diagnostic` carries a real `Span`, converted here to LSP's
//! 0-indexed line/character convention. The `location` symbolic
//! label (e.g. `"ClassName::methodName"`) is still folded into the
//! message text rather than dropped - LSP has no field for it. Known
//! remaining imprecision: LSP wants UTF-16 code-unit columns, and this
//! project's `Span` counts UTF-8 characters - an acceptable approximation
//! for now (both agree for any line without astral-plane characters, the
//! overwhelming majority of real PHP source), not a correctness bug worth
//! blocking on.

use rustc_hash::FxHashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
    Range as LspRange, ServerCapabilities, ServerInfo, SignatureHelp, SignatureHelpOptions,
    SignatureHelpParams, SignatureInformation, SymbolInformation, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, WorkDoneProgress, WorkDoneProgressBegin, WorkDoneProgressEnd,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use crate::checker::Checker;
use crate::diagnostic;
use crate::project::Project;
use crate::project::use_sites;
use crate::registry::{FunctionSig, format_signature, param_label};
use crate::{baseline, ignore_errors, restricted_symbols};
use syntax::ast::ClassKind;
use syntax::token::Span;
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
mod workspace_config;
mod workspace_symbol;

#[cfg(test)]
use protocol::percent_decode;
use protocol::{
    diagnostic_fix_to_code_action, from_lsp_position, log_message_notification, path_to_uri,
    progress_notification, publish_diagnostics_notification, pump_messages, ranges_overlap,
    to_lsp_range, to_request_id, uri_to_path, write_message,
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
        Type::Object(class, _) => out.push(class.clone().to_string()),
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
    // The last-computed, post-`apply_post_filters` diagnostic set for
    // every path this server has ever checked - `code_actions` reads
    // this instead of re-running a whole-file checker pass per request
    // (a real editor's lightbulb fires `textDocument/codeAction` far
    // more often than actual edits, e.g. on cursor movement). Kept in
    // sync by every diagnostic-producing path (`check_all_analyzed_paths`,
    // `publish_diagnostics_for`), cleared for a path `remove_workspace_folders`/
    // `handle_watched_files_changed`'s Deleted branch drop. Safe under
    // today's fully synchronous, one-message-at-a-time `run_loop`: a
    // `codeAction` request can only ever see whatever the most recently
    // processed `didChange`/scan already committed, never a torn or
    // stale-relative-to-an-earlier-unprocessed-edit state.
    last_diagnostics: FxHashMap<PathBuf, Vec<diagnostic::Diagnostic>>,
    // The exact current text `didOpen`/`didChange` last sent for a path,
    // regardless of whether it parses - `Project::source`/`Project::ast`
    // only ever reflect the last *successfully parsed* version, but
    // `textDocument/completion`'s member-completion path needs the LIVE
    // (very often mid-edit and unparseable - `$x->` alone is a syntax
    // error) buffer text to tokenize and to extract a receiver
    // expression's own text from. See `completion`'s own module doc
    // comment for the full mechanism this feeds.
    latest_text: FxHashMap<PathBuf, String>,
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
    // Every workspace folder root already loaded - `add_workspace_folders`/
    // `remove_workspace_folders` dedup/filter against this.
    workspace_roots: Vec<PathBuf>,
    // The most recently configured workspace root, kept around so
    // `apply_post_filters` can rewrite a diagnostic's path to root-
    // relative before consulting `ignore_error_entries`/`baseline` - see
    // `configure_from_workspace`. With more than one workspace folder,
    // this (and `baseline`/`ignore_error_entries`/`level_cutoff` below)
    // are last-root-wins: `Project`/`Registry` have no per-root config
    // scoping today, matching 1.1's own already-accepted single-
    // workspace-config limitation, generalized rather than newly
    // introduced here.
    root: Option<PathBuf>,
    baseline: Option<crate::baseline::Baseline>,
    ignore_error_entries: Vec<crate::ignore_errors::IgnoreErrorEntry>,
    // The same `Option<u8>` `Project::set_configured_level` also
    // received - kept separately here too since `Project`/`Registry`
    // only use it for type-strictness, not for filtering already-
    // produced diagnostics before they're published.
    level_cutoff: Option<u8>,
    // A `phpstan-rs.configPath` setting override, applied on top of
    // `root`'s own default `phpstan-rs.toml` lookup by every subsequent
    // `configure_from_workspace`/`reload_configuration` call. `None`
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
    // The last `ResolvedWorkspaceConfig` actually applied to
    // `self.project`/`self` - `reload_configuration`'s no-op fast path
    // compares a freshly resolved value against this via real `PartialEq`
    // (never a `Debug`-formatted fingerprint - `SqlReflectionConfig`'s own
    // `Debug` impl always prints its `password` field as a constant
    // redacted token, which would make a password-only edit invisible to
    // a string-based comparison).
    last_applied_config: Option<workspace_config::ResolvedWorkspaceConfig>,
    // The last resolved config a "this needs a window reload" warning was
    // already emitted for (see `reload_configuration`'s file-set-affecting
    // check) - distinct from `last_applied_config` so the warning doesn't
    // re-fire on every subsequent unrelated settings save while the same
    // unapplied change sits on disk.
    last_warned_pending_config: Option<workspace_config::ResolvedWorkspaceConfig>,
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
    inlay_hint_cache: FxHashMap<PathBuf, (u64, Vec<(Span, Arc<str>)>, Vec<(Span, Type)>)>,
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
            open_uris: FxHashMap::default(),
            last_diagnostics: FxHashMap::default(),
            latest_text: FxHashMap::default(),
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
            baseline: None,
            ignore_error_entries: Vec::new(),
            level_cutoff: None,
            config_path_override: None,
            enabled: true,
            supports_configuration_pull: false,
            last_applied_config: None,
            last_warned_pending_config: None,
            supports_watched_files: false,
            file_versions: FxHashMap::default(),
            inlay_hint_cache: FxHashMap::default(),
        }
    }

    /// Resolves `root`'s own `phpstan-rs.toml` and applies it to
    /// `self.project`/`self`, so a workspace's config produces the same
    /// `Project` state (and the same published-diagnostic set) `check-
    /// project` would build/report for the identical tree. Returns the
    /// context paths (`vendor`/`scan_paths`/`stub_paths`) `load_workspace`
    /// needs to load before walking `root` itself.
    fn configure_from_workspace(&mut self, root: &Path) -> workspace_config::WorkspaceContextPaths {
        let resolved = workspace_config::resolve(root, self.config_path_override.as_deref());
        self.apply_resolved_config(root, resolved)
    }

    /// The apply half of `configure_from_workspace`, factored out so
    /// `reload_configuration` can apply an already-resolved value (its own
    /// no-op fast path needs the resolved value in hand *before* deciding
    /// whether to apply it, which `configure_from_workspace`'s
    /// resolve-then-apply-in-one-call shape doesn't allow). Stores a clone
    /// of `resolved` into `self.last_applied_config` before any of its
    /// fields are moved out below.
    fn apply_resolved_config(
        &mut self,
        root: &Path,
        resolved: workspace_config::ResolvedWorkspaceConfig,
    ) -> workspace_config::WorkspaceContextPaths {
        self.last_applied_config = Some(resolved.clone());
        self.project
            .set_strict_mixed_arrays(resolved.strict_mixed_arrays);
        self.project
            .set_check_uninitialized_properties(resolved.check_uninitialized_properties);
        self.project
            .set_check_internal_usage(resolved.check_internal_usage);
        self.project
            .set_check_function_name_case(resolved.check_function_name_case);
        self.project
            .set_check_deprecated_usage(resolved.check_deprecated_usage);
        self.project
            .set_check_empty_method_bodies(resolved.check_empty_method_bodies);
        self.project
            .set_check_missing_checked_exceptions(resolved.check_missing_checked_exceptions);
        self.project.set_check_strict_printf_placeholder_types(
            resolved.check_strict_printf_placeholder_types,
        );
        self.project
            .set_check_too_wide_throw_type_in_protected_and_public_methods(
                resolved.check_too_wide_throw_type_in_protected_and_public_methods,
            );
        self.project
            .set_check_too_wide_return_type_in_protected_and_public_methods(
                resolved.check_too_wide_return_type_in_protected_and_public_methods,
            );
        self.project
            .set_check_too_wide_by_ref_type_in_protected_and_public_methods(
                resolved.check_too_wide_by_ref_type_in_protected_and_public_methods,
            );
        self.project
            .set_check_method_throw_type_covariance(resolved.check_method_throw_type_covariance);
        self.project
            .set_check_benevolent_union_types(resolved.check_benevolent_union_types);
        self.project
            .set_check_missing_callable_signature(resolved.check_missing_callable_signature);
        self.project.set_check_duplicate_function_declarations(
            resolved.check_duplicate_function_declarations,
        );
        self.project
            .set_check_duplicate_class_declarations(resolved.check_duplicate_class_declarations);
        self.project
            .set_check_boolean_trap(resolved.check_boolean_trap);
        self.project
            .set_check_array_key_cast(resolved.check_array_key_cast);
        self.project
            .set_check_parent_constructor_call(resolved.check_parent_constructor_call);
        self.project
            .set_check_promoted_property_suggestions(resolved.check_promoted_property_suggestions);
        self.project
            .set_check_boolean_conditions(resolved.check_boolean_conditions);
        self.project
            .set_check_dynamic_properties(resolved.check_dynamic_properties);
        self.project
            .set_check_swapped_arguments(resolved.check_swapped_arguments);
        self.project
            .set_check_never_return_suggestions(resolved.check_never_return_suggestions);
        self.project
            .set_check_switch_missing_default(resolved.check_switch_missing_default);
        self.project
            .set_check_throw_without_message(resolved.check_throw_without_message);
        self.project
            .set_check_throw_base_class(resolved.check_throw_base_class);
        self.project
            .set_check_exception_hygiene(resolved.check_exception_hygiene);
        self.project
            .set_check_current_time_dependence(resolved.check_current_time_dependence);
        self.project
            .set_check_psr3_exception_context(resolved.check_psr3_exception_context);
        self.project
            .set_check_psr3_logging(resolved.check_psr3_logging);
        self.project.set_psr3_context_key_pattern(
            resolved
                .psr3_context_key_pattern
                .as_deref()
                .and_then(|pattern| regex::Regex::new(pattern).ok()),
        );
        self.project
            .set_check_magic_numbers(resolved.check_magic_numbers);
        self.project.set_magic_number_options(
            resolved.magic_number_exemptions.clone(),
            resolved.magic_numbers_ignore_numeric_strings,
        );
        self.project
            .set_check_native_type_hints(resolved.check_native_type_hints);
        self.project
            .set_check_implicit_array_creation(resolved.check_implicit_array_creation);
        self.project
            .set_check_useless_casts(resolved.check_useless_casts);
        self.project
            .set_check_strict_array_filter(resolved.check_strict_array_filter);
        self.project
            .set_check_mixed_type_operands(resolved.check_mixed_type_operands);
        self.project
            .set_check_comparable_types(resolved.check_comparable_types);
        self.project
            .set_check_arithmetic_operands(resolved.check_arithmetic_operands);
        self.project
            .set_check_identity_vs_equals(resolved.check_identity_vs_equals);
        self.project
            .set_check_loose_haystack_comparison(resolved.check_loose_haystack_comparison);
        self.project
            .set_check_strict_function_calls(resolved.check_strict_function_calls);
        self.project
            .set_check_switch_case_types(resolved.check_switch_case_types);
        self.project
            .set_check_loop_variable_overwrite(resolved.check_loop_variable_overwrite);
        self.project
            .set_check_dynamic_static_calls(resolved.check_dynamic_static_calls);
        self.project
            .set_check_illegal_constructor_calls(resolved.check_illegal_constructor_calls);
        self.project
            .set_check_enum_exhaustiveness(resolved.check_enum_exhaustiveness);
        self.project
            .set_check_null_coercion(resolved.check_null_coercion);
        self.project
            .set_check_new_exception_discarded(resolved.check_new_exception_discarded);
        self.project
            .set_check_risky_tests(resolved.check_risky_tests);
        self.project.set_configured_level(resolved.configured_level);
        self.project
            .set_restricted_symbols(&resolved.restricted_symbol_entries);
        self.project
            .set_dynamic_constant_names(&resolved.dynamic_constant_names);
        self.project
            .set_doctrine_custom_functions(&resolved.doctrine_custom_function_entries);
        self.project
            .set_sql_reflection_config(resolved.sql_reflection_config);
        self.project
            .set_sql_reflection_schema(resolved.sql_reflection_schema);
        self.project.set_php_version(resolved.php_version);
        self.project
            .set_assertion_tables(resolved.assertion_entries, resolved.class_argument_returns);
        self.project
            .set_phpstan_api_check_enabled(resolved.phpstan_api_check_enabled);

        self.root = Some(root.to_path_buf());
        self.baseline = resolved.baseline;
        self.ignore_error_entries = resolved.ignore_error_entries;
        self.level_cutoff = resolved.configured_level;

        workspace_config::WorkspaceContextPaths {
            vendor: resolved.vendor_context,
            extra_dirs: resolved.extra_context_dirs,
            analyze_paths: resolved.analyze_paths,
        }
    }

    /// Applies the same severity-level cutoff / `ignore_errors` /
    /// baseline post-filters `check-project` applies (in that order),
    /// but scoped to whatever partial result set `update_file`'s own
    /// transitive-dependents pass just produced. Never surfaces
    /// `baseline::filter`'s stale-entry report here: its staleness
    /// detection assumes it's run against the whole project's
    /// diagnostics at once, and treating every other baselined file as
    /// "no longer matches" on every keystroke would be a constant
    /// false-positive warning storm. `ignore_errors`/`baseline` both
    /// match against a root-relative path string, so results are
    /// rewritten to root-relative and back around each - a reasonable
    /// default, not a provable-correct one, for either.
    fn apply_post_filters(
        &self,
        mut results: FxHashMap<PathBuf, Vec<diagnostic::Diagnostic>>,
    ) -> FxHashMap<PathBuf, Vec<diagnostic::Diagnostic>> {
        if let Some(cutoff) = self.level_cutoff {
            for diagnostics in results.values_mut() {
                diagnostics.retain(|d| d.level <= cutoff);
            }
        }
        if self.ignore_error_entries.is_empty() && self.baseline.is_none() {
            return results;
        }
        let Some(root) = &self.root else {
            return results;
        };
        let to_relative = |path: PathBuf| -> PathBuf {
            path.strip_prefix(root)
                .map(Path::to_path_buf)
                .unwrap_or(path)
        };
        let mut relative: FxHashMap<PathBuf, Vec<diagnostic::Diagnostic>> = results
            .into_iter()
            .map(|(path, diags)| (to_relative(path), diags))
            .collect();
        if !self.ignore_error_entries.is_empty() {
            ignore_errors::filter(&mut relative, &self.ignore_error_entries);
        }
        restricted_symbols::filter_by_path(&mut relative);
        if let Some(baseline) = &self.baseline {
            let _stale = baseline::filter(&mut relative, baseline);
        }
        relative
            .into_iter()
            .map(|(path, diags)| (root.join(path), diags))
            .collect()
    }

    /// Reads and parses one file, dispatching to `Project::load_file`
    /// (context: vendor/scan/stub, never analyzed or reported) or
    /// `Project::load_analyzed_file` (a real, first-party file under
    /// `root`) based on `analyzed`. A parse failure is logged (both to
    /// stderr and as a `window/logMessage` notification) and skipped,
    /// not fatal to the rest of the scan.
    fn load_one_file(&mut self, path: PathBuf, analyzed: bool, messages: &mut Vec<Message>) {
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let source = String::from_utf8_lossy(&bytes).into_owned();
        let path_for_log = path.clone();
        let result = if analyzed {
            self.project.load_analyzed_file(path, &source)
        } else {
            self.project.load_file(path, &source)
        };
        if let Err(e) = result {
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

    /// Loads `context`'s vendor/scan/stub paths first (`analyzed:
    /// false`), then every `.php` file under `context.analyze_paths`
    /// (falling back to the whole of `root` when the config sets no
    /// `paths` at all - see `WorkspaceContextPaths::analyze_paths`'s own
    /// doc comment) as `analyzed: true` (skipping any path already loaded
    /// as context or by an earlier call,
    /// e.g. a prior workspace folder, or an already-`didOpen`ed file),
    /// so the cross-file registry and dependency graph this whole phase
    /// is about are populated from the very first diagnostic, not built
    /// up one `didOpen` at a time. Deliberately does NOT call
    /// `Project::finish_loading` itself - `add_workspace_folders` calls
    /// it once for a whole batch of roots, not once per root here; a
    /// bare call to this method with no `finish_loading` afterward would
    /// leave the registry's derived state (trait flattening, ancestor
    /// names, ...) stale for whatever it just loaded.
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
    fn load_workspace_files(
        &mut self,
        root: &Path,
        context: &workspace_config::WorkspaceContextPaths,
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut loaded: rustc_hash::FxHashSet<PathBuf> = rustc_hash::FxHashSet::default();

        let mut context_files = context.vendor.files.clone();
        for dir in context.vendor.dirs.iter().chain(context.extra_dirs.iter()) {
            crate::project::collect_php_files(dir, &mut context_files);
        }
        context_files.sort();
        context_files.dedup();
        for path in context_files {
            let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !loaded.insert(canonical) {
                continue;
            }
            self.load_one_file(path, false, &mut messages);
        }

        let mut files = Vec::new();
        if context.analyze_paths.is_empty() {
            crate::project::collect_php_files_excluding(
                root,
                crate::project::DEFAULT_SCAN_EXCLUDED_DIR_NAMES,
                &mut files,
            );
        } else {
            // A configured `paths` is an include-only allowlist, same as
            // `run_check_project`'s own `collect_analyze_and_context_files`
            // - no `DEFAULT_SCAN_EXCLUDED_DIR_NAMES` floor applies on top
            // of it (an explicitly-listed `vendor/`-named entry, unlikely
            // but real, would otherwise be silently dropped again here).
            for path in &context.analyze_paths {
                crate::project::collect_php_files(path, &mut files);
            }
        }
        // See main.rs's run_check_project for why this matters: a real
        // workspace can genuinely declare the same fully-qualified class
        // twice (duplicate vendored copies of a library), and
        // unsorted readdir order would make which one wins the
        // registry slot filesystem-dependent.
        files.sort();
        for path in files {
            let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !loaded.insert(canonical) {
                continue;
            }
            // A non-conformant client that sends didOpen/didChange for a
            // path already loaded before "initialized" arrives - this
            // scan must not re-register a file update_file's own caller
            // already indexed, since load_file has no idempotency guard
            // of its own.
            if self.project.ast(&path).is_some() {
                continue;
            }
            self.load_one_file(path, true, &mut messages);
        }
        messages
    }

    /// Loads every not-yet-tracked root in `roots` (deduped against
    /// `self.workspace_roots`) via `configure_from_workspace` +
    /// `load_workspace_files`, then runs `Project::finish_loading` at
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
        let mut any_new = false;
        for root in roots {
            if self.workspace_roots.contains(root) {
                continue;
            }
            // Config resolution (a workspace's own phpstan-rs.toml,
            // Composer vendor discovery) is genuinely per-root - but its
            // *effect* on `self`/`self.project` (opt-in checks, php
            // version, the post-filter fields `apply_post_filters`
            // reads) is last-root-wins for now: `Project`/`Registry`
            // have no per-root config scoping, only one shared instance
            // for however many folders are open. Matches 1.1's own
            // already-accepted single-workspace-config limitation,
            // generalized rather than newly introduced here.
            let context = self.configure_from_workspace(root);
            messages.extend(self.load_workspace_files(root, &context));
            self.workspace_roots.push(root.clone());
            any_new = true;
        }
        if any_new {
            self.project.finish_loading();
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
        self.workspace_roots.retain(|r| !roots.contains(r));
        let paths: Vec<PathBuf> = self
            .project
            .loaded_paths()
            .filter(|p| roots.iter().any(|root| p.starts_with(root)))
            .map(Path::to_path_buf)
            .collect();

        let mut messages: Vec<Message> = paths
            .iter()
            .filter_map(|p| self.open_uris.remove(p))
            .map(|(uri, _version)| publish_diagnostics_notification(uri, None, &[]))
            .collect();
        for p in &paths {
            self.last_diagnostics.remove(p);
            self.latest_text.remove(p);
        }

        let results = self.project.remove_files(&paths);
        messages.extend(self.publish_diagnostics_for(results));
        messages
    }

    /// Runs the checker over every analyzed (first-party) file currently
    /// loaded, post-filtered the same way any other published diagnostic
    /// set is. Runs on a dedicated 64MiB-stack scoped thread (mirrors
    /// `main/check.rs`'s own `WORKER_STACK_SIZE` precedent) since the
    /// checker's recursive AST walk needs real headroom over the ~2MiB a
    /// bare thread gets, and through `Project::check_paths_isolating_panics`
    /// (not `check_paths`) so one file's latent checker bug can't take
    /// down the whole push. Shared by `publish_workspace_diagnostics`
    /// (which then drops empty results) and `republish_known_files`
    /// (which must not, so a file previously flagged and now clean gets
    /// an explicit clearing notification).
    fn check_all_analyzed_paths(&mut self) -> FxHashMap<PathBuf, Vec<diagnostic::Diagnostic>> {
        let mut paths = self.project.analyzed_paths();
        if paths.is_empty() {
            return FxHashMap::default();
        }
        // Deterministic order (matches load_workspace's own `files.sort()`
        // rationale) - HashMap/HashSet iteration order would otherwise
        // make repeated runs against an unchanged workspace look like a
        // spurious diff to anyone comparing published-message order.
        paths.sort();
        const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;
        let (results, panics) = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(WORKER_STACK_SIZE)
                .spawn_scoped(scope, || self.project.check_paths_isolating_panics(&paths))
                .expect("failed to spawn worker thread")
                .join()
                .unwrap_or_default()
        });
        for (path, msg) in &panics {
            eprintln!(
                "phpstan-rs lsp: checking {} panicked: {msg}",
                path.display()
            );
        }
        let results = self.apply_post_filters(results);
        self.last_diagnostics
            .extend(results.iter().map(|(p, diags)| (p.clone(), diags.clone())));
        results
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

    /// Publishes a `textDocument/publishDiagnostics` notification for
    /// every analyzed file with findings - without this, a freshly opened
    /// workspace shows an empty Problems panel until each affected file
    /// happens to be individually opened or edited, which for most files
    /// in a real project never happens. Empty results are dropped: at an
    /// initial load nothing has been published for these paths yet, so
    /// there is nothing to clear.
    fn publish_workspace_diagnostics(&mut self) -> Vec<Message> {
        self.check_all_analyzed_paths()
            .into_iter()
            .filter(|(_, diags)| !diags.is_empty())
            .map(|(p, diags)| {
                let (uri, version) = self.uri_and_version_for(&p);
                publish_diagnostics_notification(uri, version, &diags)
            })
            .collect()
    }

    /// `reload_configuration`'s own republish step, once it decides
    /// something actually changed. Unlike `publish_workspace_diagnostics`,
    /// this never drops an empty result: a file that had diagnostics
    /// under the old config and none under the new one needs an explicit
    /// empty `publishDiagnostics` to actually clear them client-side, not
    /// silence. While `!self.enabled`, skips rechecking entirely and just
    /// clears every currently-open file - phpstan-rs.enable means "stop
    /// showing squiggles", not "stop the language server".
    fn republish_known_files(&mut self) -> Vec<Message> {
        if !self.enabled {
            return self
                .open_uris
                .values()
                .cloned()
                .map(|(uri, version)| publish_diagnostics_notification(uri, version, &[]))
                .collect();
        }
        self.check_all_analyzed_paths()
            .into_iter()
            .map(|(p, diags)| {
                let (uri, version) = self.uri_and_version_for(&p);
                publish_diagnostics_notification(uri, version, &diags)
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
        self.publish_diagnostics_for(self.apply_post_filters(results))
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
        should_abort: impl FnMut() -> bool,
    ) -> Vec<Message> {
        *self.file_versions.entry(path.to_path_buf()).or_insert(0) += 1;
        self.open_uris
            .insert(path.to_path_buf(), (uri.clone(), Some(version)));
        self.latest_text
            .insert(path.to_path_buf(), text.to_string());
        let results = match self
            .project
            .update_file_cancellable(path, text, should_abort)
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
        self.publish_diagnostics_for(self.apply_post_filters(results))
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
            last_by_path.insert(path, (change.uri, change.typ));
        }

        let mut messages = Vec::new();
        for (path, (uri, typ)) in last_by_path {
            if typ == lsp_types::FileChangeType::DELETED {
                self.open_uris.remove(&path);
                self.last_diagnostics.remove(&path);
                self.latest_text.remove(&path);
                let results = self.project.remove_file(&path);
                messages.extend(self.publish_diagnostics_for(self.apply_post_filters(results)));
                continue;
            }
            // Created and Changed both just need the path's current
            // on-disk content reflected - `update_file` already tolerates
            // a path with no prior entry (a brand-new file), so there is
            // no separate load-vs-update branch to make here. A read
            // failure (raced with a subsequent delete before this ran, a
            // transient permission error, ...) is skipped silently: a
            // later Deleted event, if any, cleans it up properly.
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // No document version exists for an externally-observed
            // change (there is no didOpen/didChange behind it) - `None`
            // matches load_workspace's own "never opened" default.
            self.open_uris.insert(path.clone(), (uri, None));
            self.latest_text.insert(path.clone(), text.clone());
            match self.project.update_file(&path, &text) {
                Ok(results) => {
                    messages.extend(self.publish_diagnostics_for(self.apply_post_filters(results)));
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
        results: FxHashMap<PathBuf, Vec<diagnostic::Diagnostic>>,
    ) -> Vec<Message> {
        results
            .into_iter()
            .map(|(p, diags)| {
                let (dep_uri, dep_version) = self.uri_and_version_for(&p);
                self.last_diagnostics.insert(p, diags.clone());
                publish_diagnostics_notification(dep_uri, dep_version, &diags)
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
        let mut checker = Checker::new(self.project.registry()).with_sql(self.project.sql());
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
        let mut checker = Checker::new(self.project.registry()).with_sql(self.project.sql());
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

    /// `textDocument/typeDefinition` (Phase 108's AM2) - jumps from an
    /// inferred `Type` to that type's own class declaration(s), reusing
    /// the identical throwaway-`Checker` probe/`type_to_classes` pair
    /// `format_hover`'s `Instance`/`VariableSelf`/`VariableRead` arms
    /// already use to answer "what type is at this span" - the only new
    /// work is resolving each class name the `Type` names to its own
    /// `decl_locations` entry, instead of formatting the `Type` as text.
    /// `Class`/`Function`/`Member` decline (`None`): none of them is a
    /// value with an inferred type to jump *from* (a class/function name
    /// reference already resolves to itself via ordinary
    /// `textDocument/definition`; a class member reference alone doesn't
    /// disambiguate a receiver type the way `Instance` does), matching
    /// `rename`'s own precedent of scoping a new capability narrower
    /// than every `Resolved` variant in its v1.
    fn type_definition(&self, uri: &Uri, position: LspPosition) -> Option<GotoDefinitionResponse> {
        let path = uri_to_path(uri)?;
        let resolved = navigation::resolve_at(&self.project, &path, from_lsp_position(position))?;
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
        let mut classes = Vec::new();
        type_to_classes(&ty, &mut classes);
        let locations = self.project.decl_locations();
        let mut locs: Vec<Location> = Vec::new();
        for class in &classes {
            if let Some((target_path, span)) = locations.classes.get(class) {
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

    fn document_symbol(&self, uri: &Uri) -> Option<DocumentSymbolResponse> {
        let path = uri_to_path(uri)?;
        let stmts = self.project.ast(&path)?;
        Some(DocumentSymbolResponse::Nested(
            document_symbol::document_symbols(stmts),
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
        Some(lsp_types::SemanticTokensResult::Tokens(
            lsp_types::SemanticTokens {
                result_id: None,
                data: semantic_tokens::encode(tokens),
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
    /// from `decl_locations` (already built for hover/definition, so no
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
        let resolved = navigation::resolve_at(&self.project, &path, from_lsp_position(position))?;
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
            let decl_locations = self.project.decl_locations();
            for target in &targets {
                let decl = match target {
                    use_sites::RefTarget::Class(fqn) => decl_locations.classes.get(fqn),
                    use_sites::RefTarget::Function(fqn) => decl_locations.functions.get(fqn),
                    use_sites::RefTarget::Member(owner, name) => {
                        decl_locations.members.get(&(owner.clone(), name.clone()))
                    }
                };
                if let Some((p, span)) = decl {
                    locations.push(self.location(p, *span));
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
        let path = uri_to_path(uri)?;
        let resolved = navigation::resolve_at(&self.project, &path, from_lsp_position(position))?;
        let (target, decl) = match &resolved {
            navigation::Resolved::Class(fqn) => (
                use_sites::RefTarget::Class(fqn.clone()),
                self.project
                    .decl_locations()
                    .classes
                    .get(fqn)
                    .map(|(p, s)| (p.clone(), *s)),
            ),
            navigation::Resolved::Function(fqn) => (
                use_sites::RefTarget::Function(fqn.clone()),
                self.project
                    .decl_locations()
                    .functions
                    .get(fqn)
                    .map(|(p, s)| (p.clone(), *s)),
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
                    range: to_lsp_range(*span),
                    new_text: new_name.to_string(),
                });
        }
        if let Some((p, span)) = decl {
            let (edit_uri, _) = self.uri_and_version_for(&p);
            changes
                .entry(edit_uri)
                .or_default()
                .push(lsp_types::TextEdit {
                    range: to_lsp_range(span),
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
        let path = uri_to_path(uri)?;
        let target = navigation::resolve_at(&self.project, &path, from_lsp_position(position))?;
        let mut spans: Vec<Span> = navigation::collect_all(&self.project, &path)
            .into_iter()
            .filter(|(_, r)| self.same_symbol(&target, r))
            .map(|(span, _)| span)
            .collect();
        spans.sort_by_key(|s| (s.start.line, s.start.col, s.end.line, s.end.col));
        spans.dedup();
        let highlights: Vec<DocumentHighlight> = spans
            .into_iter()
            .map(|span| DocumentHighlight {
                range: to_lsp_range(span),
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
        let hints = inlay_hints::filter_to_range(&cached.0, &cached.1, range);
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
        let pos = from_lsp_position(position);
        let help = signature_help::resolve_at(&self.project, &path, pos)?;
        self.build_signature_help(help)
    }

    /// Resolves `help.target` into one or more real `FunctionSig`s (more
    /// than one only for `Instance` on a `Union`/`Intersection`
    /// receiver, the same multi-owner case `format_member_hover_multi`
    /// already handles for hover) and builds the LSP response.
    /// `activeParameter` is set on every returned `SignatureInformation`
    /// (not just the top-level `SignatureHelp.activeParameter`, which
    /// only ever describes signature 0) so a client honors it correctly
    /// regardless of which signature it's currently displaying.
    fn build_signature_help(&self, help: signature_help::SigHelp) -> Option<SignatureHelp> {
        let registry = self.project.registry();
        let signatures: Vec<(String, &FunctionSig)> = match &help.target {
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
                let sigs: Vec<(String, &FunctionSig)> = owners
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
                let active_parameter = if sig.params.is_empty() {
                    None
                } else {
                    Some(help.active_parameter.min(sig.params.len() - 1) as u32)
                };
                SignatureInformation {
                    label: format_signature(&name, sig),
                    documentation: None,
                    parameters: Some(
                        sig.params
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
        let stmts = self.project.ast(&path)?;
        let tokens = completion::tokenize(&text);
        let cursor = from_lsp_position(position);
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
            ))),
        }
    }

    /// Quick fixes for whatever diagnostics in `path` overlap `range` and
    /// carry a `DiagnosticFix`. Reads `self.last_diagnostics` rather than
    /// re-running the checker - see that field's own doc comment for why.
    /// A cache miss (a workspace file requested before any `didOpen`/
    /// `didChange`/scan ever populated it) is a one-time, self-healing
    /// fallback: computed once via `Project::check_file`, post-filtered
    /// the same way every other diagnostic set is, then cached.
    fn code_actions(&mut self, uri: &Uri, range: LspRange) -> Option<CodeActionResponse> {
        let path = uri_to_path(uri)?;
        if !self.last_diagnostics.contains_key(&path) {
            let raw = self.project.check_file(&path);
            let filtered = self.apply_post_filters(FxHashMap::from_iter([(path.clone(), raw)]));
            let diags = filtered.into_values().next().unwrap_or_default();
            self.last_diagnostics.insert(path.clone(), diags);
        }
        let diagnostics = self.last_diagnostics.get(&path)?;
        let source = self.project.source(&path);
        Some(
            diagnostics
                .iter()
                .filter(|d| d.fix.is_some() && ranges_overlap(to_lsp_range(d.span), range))
                .filter_map(|d| diagnostic_fix_to_code_action(uri, d, source))
                .collect(),
        )
    }

    fn location(&self, target_path: &Path, span: Span) -> Location {
        let (target_uri, _version) = self.uri_and_version_for(target_path);
        Location {
            uri: target_uri,
            range: to_lsp_range(span),
        }
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
    /// when relative) and republishes whatever changed.
    ///
    /// A true no-op - the freshly resolved config and `enable` flag both
    /// match what's already applied - returns `Vec::new()` without
    /// re-checking anything, since this notification fires for ANY client
    /// settings save under the registered section, not just a
    /// `phpstan-rs` one; re-checking every analyzed file on every
    /// unrelated settings.json save would be a real, common-case cost.
    ///
    /// `scan_paths`/`stub_paths`/`no_composer` changes are detected
    /// separately (via the resolved `vendor_context`/`extra_context_dirs`)
    /// but not specially skipped: `Project` has no unload/reset primitive,
    /// so the already-loaded file set simply stays what it was, while
    /// every other (hot-appliable) field still applies immediately. A
    /// one-time `window/logMessage` (deduped via `last_warned_pending_config`)
    /// tells the user the file set itself needs a window reload to catch
    /// up - it is not silently wrong, just stale until then.
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
        let resolved = workspace_config::resolve(&root, config_path_override.as_deref());
        if settings.enable == was_enabled && self.last_applied_config.as_ref() == Some(&resolved) {
            return Vec::new();
        }

        let file_set_changed = self.last_applied_config.as_ref().is_some_and(|prev| {
            prev.vendor_context != resolved.vendor_context
                || prev.extra_context_dirs != resolved.extra_context_dirs
        });

        self.enabled = settings.enable;
        self.config_path_override = config_path_override;
        self.apply_resolved_config(&root, resolved);

        let mut messages = Vec::new();
        if file_set_changed
            && self.last_warned_pending_config.as_ref() != self.last_applied_config.as_ref()
        {
            self.last_warned_pending_config = self.last_applied_config.clone();
            messages.push(log_message_notification(
                MessageType::WARNING,
                "phpstan-rs: scan_paths/stub_paths/no_composer changed - reload the window to pick up the new file set".to_string(),
            ));
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
/// comment).
pub fn run() -> i32 {
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
    run_loop(&rx, writer)
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
fn run_loop<W: Write + Send + 'static>(receiver: &Receiver<Message>, writer: Arc<Mutex<W>>) -> i32 {
    let server = Arc::new(Mutex::new(Server::new()));
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
/// "hover/definition block behind a slow recheck" tradeoff), then
/// re-confirms against `debounce.queue` that this edit is STILL the
/// freshest one recorded for its path before publishing - covers the
/// small window between the cascade's own last per-file `should_abort`
/// check and its actual return. A `should_abort` closure captures the
/// edit's own generation and re-locks `debounce.queue` (never `server`)
/// on each per-file check - the single point where this worker thread
/// nests both locks, always in the same Server-then-Debounce order.
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
                    // A real vscode-languageclient is expected to
                    // advertise general.positionEncodings. This server's
                    // own Span already counts UTF-8 characters (see this
                    // module's top-of-file doc comment), so echoing
                    // "utf-8" back when the client offers it makes that
                    // existing counting exactly correct - whenever it's
                    // offered at all, not only when it's the client's
                    // top preference, since utf-8 is the only encoding
                    // this server actually implements. Absent an offer,
                    // the spec's utf-16 default applies and today's
                    // approximation is unchanged.
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
                    text_document_sync: Some(TextDocumentSyncCapability::Kind(
                        TextDocumentSyncKind::FULL,
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
                    version: Some(env!("CARGO_PKG_VERSION").to_string()),
                }),
            };
            Message::Response(Response::new_ok(id.clone(), result))
        }
        "shutdown" => {
            server.shutdown_received = true;
            Message::Response(Response::new_ok(id.clone(), serde_json::Value::Null))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
        }
        "textDocument/documentSymbol" => {
            let result = serde_json::from_value::<DocumentSymbolParams>(req.params)
                .ok()
                .and_then(|params| server.document_symbol(&params.text_document.uri))
                .map(|symbols| serde_json::to_value(symbols).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
        }
        "textDocument/semanticTokens/full" => {
            let result = serde_json::from_value::<lsp_types::SemanticTokensParams>(req.params)
                .ok()
                .and_then(|params| server.semantic_tokens_full(&params.text_document.uri))
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            Message::Response(Response::new_ok(id.clone(), result))
        }
        "textDocument/inlayHint" => {
            let result = serde_json::from_value::<InlayHintParams>(req.params)
                .ok()
                .and_then(|params| server.inlay_hints(&params.text_document.uri, params.range))
                .map(|response| serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
            Message::Response(Response::new_ok(id.clone(), result))
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
