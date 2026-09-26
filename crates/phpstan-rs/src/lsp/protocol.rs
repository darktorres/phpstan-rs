use std::io::{self, BufRead, Write};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use crossbeam_channel::Sender;
use lsp_server::{Message, Notification as LspNotification, RequestId};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic as LspDiagnostic,
    DiagnosticSeverity, LogMessageParams, MessageType, NumberOrString, Position as LspPosition,
    ProgressParams, ProgressParamsValue, ProgressToken, PublishDiagnosticsParams,
    Range as LspRange, TextEdit, Uri, WorkDoneProgress, WorkspaceEdit,
};

use diagnostics::{Diagnostic, diagnostic_order};
use syntax::token::{Position, Span};

/// The outcome of trying to read one framed LSP message. `Eof` (a clean
/// transport disconnection) and `Disconnected` (a real I/O failure
/// reading the header/body - the stream's position afterward is
/// unknown) both mean "stop the loop". `MalformedBody` is the one
/// genuinely recoverable case: `lsp_server::Message::read` reads exactly
/// `Content-Length` bytes before ever attempting to parse them as JSON
/// (confirmed against its own source), so a body that fails to parse
/// still leaves the reader correctly positioned at the start of the
/// *next* message - only this one message needs to be skipped, not the
/// whole connection torn down. `lsp_server` reports both this and a
/// genuinely unrecoverable framing error (e.g. a missing Content-Length
/// header, where the next message boundary truly isn't known) as the
/// same `io::ErrorKind::InvalidData` - real PHP LSP clients essentially
/// never send the latter, so treating every `InvalidData` as
/// recoverable-and-skippable (rather than as `Disconnected`) matches
/// this crate's own real behavior without losing anything this project's
/// own tests actually exercise.
pub(crate) enum ReadOutcome {
    Message(Message),
    Eof,
    Disconnected(io::Error),
    MalformedBody(io::Error),
}

pub(crate) fn read_message<R: BufRead>(reader: &mut R) -> ReadOutcome {
    match Message::read(reader) {
        Ok(Some(msg)) => ReadOutcome::Message(msg),
        Ok(None) => ReadOutcome::Eof,
        Err(e) if e.kind() == io::ErrorKind::InvalidData => ReadOutcome::MalformedBody(e),
        Err(e) => ReadOutcome::Disconnected(e),
    }
}

pub(crate) fn write_message<W: Write>(writer: &mut W, message: &Message) -> io::Result<()> {
    message.write(writer)
}

/// Reads framed messages from `reader` until EOF/disconnection or an
/// `exit` notification, forwarding each successfully parsed one onto
/// `sender`. A malformed body is skipped (logged, not fatal) - the same
/// tolerance `ReadOutcome::MalformedBody` already documents, just running
/// on its own thread now instead of inline in `run_loop`. A real I/O
/// failure (`Disconnected`) is still logged before returning, preserving
/// the exact operator-visible diagnostic `run_loop` used to print itself
/// before this function took over the actual `read_message` call.
/// Generic over `R: BufRead`, not tied to stdin, so it's directly
/// unit-testable with an in-memory `Cursor` with no real thread needed -
/// `lsp::run()` is the only caller that hands it real stdin, from a
/// dedicated OS thread.
pub(crate) fn pump_messages<R: BufRead>(reader: &mut R, sender: &Sender<Message>) {
    loop {
        match read_message(reader) {
            ReadOutcome::Message(m) => {
                let is_exit = matches!(&m, Message::Notification(n) if n.method == "exit");
                if sender.send(m).is_err() {
                    return; // run_loop's receiver was dropped; nothing left to forward to
                }
                if is_exit {
                    return;
                }
            }
            ReadOutcome::Eof => return,
            ReadOutcome::Disconnected(e) => {
                eprintln!("phpstan-rs lsp: failed to read message: {e}");
                // Best-effort: `run_loop` (which owns the writer) picks
                // this up as an ordinary message off the same channel -
                // see its `Message::Notification` arm's own
                // `window/logMessage` passthrough. If `run_loop`'s own
                // receiver is already gone the `send` just fails
                // silently, same as the real message case above.
                let _ = sender.send(log_message_notification(
                    MessageType::ERROR,
                    format!("phpstan-rs: failed to read message: {e}"),
                ));
                return;
            }
            ReadOutcome::MalformedBody(e) => {
                eprintln!("phpstan-rs lsp: skipping malformed message: {e}");
                let _ = sender.send(log_message_notification(
                    MessageType::WARNING,
                    format!("phpstan-rs: skipping malformed message: {e}"),
                ));
                continue;
            }
        }
    }
}

/// `file://`-scheme only - this server has no use for any other URI
/// scheme a client might send. `Uri::path()` is still percent-encoded
/// (fluent_uri, `lsp_types::Uri`'s own backing type, never decodes it
/// implicitly); the small `percent_decode` helper below covers the
/// common escapes a real filesystem path needs.
pub(crate) fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    if uri.scheme()?.as_str() != "file" {
        return None;
    }
    let decoded = percent_decode(uri.path().as_str());
    let decoded = match decoded.strip_prefix('/') {
        Some(rest) if starts_with_drive_letter(rest) => rest,
        _ => &decoded,
    };
    Some(PathBuf::from(decoded))
}

pub(crate) fn path_to_uri(path: &Path) -> Uri {
    let rendered = path_to_uri_path_string(path);
    let rendered = if starts_with_drive_letter(&rendered) {
        format!("/{rendered}")
    } else {
        rendered
    };
    let encoded = percent_encode_path(&rendered);
    Uri::from_str(&format!("file://{encoded}"))
        .expect("a percent-encoded absolute filesystem path is always a valid file:// URI")
}

/// `true` for a `C:`-style Windows drive letter at the very start of `s`
/// (single ASCII letter, then `:`) - used both to strip the URI's leading
/// `/` before such a path (`uri_to_path`) and to re-insert it
/// (`path_to_uri`), since `file:///C:/...` (leading slash present) is the
/// real shape every client sends, but a bare `C:/...` path string would
/// otherwise parse as a URI *authority*, not a path, once prefixed with
/// `file://`.
fn starts_with_drive_letter(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Renders `path` as a `/`-separated URI path string, component by
/// component. Deliberately not
/// `path.display().to_string().replace('\\', "/")`: that blind replace
/// corrupts a real Unix filename containing a literal `\` (legal on
/// Linux, this project's only build target) by silently turning it into
/// an extra path segment. `std::path::Component` is already
/// platform-specific internally (it only treats `\` as a separator, and
/// ever yields `Prefix`, when actually compiled for Windows), so joining
/// components with `/` gets Windows-correct behavior for free on a
/// Windows build with no `cfg(windows)` branch of our own, while never
/// touching a literal `\` inside one `Normal` component on this
/// project's real (Linux) target.
fn path_to_uri_path_string(path: &Path) -> String {
    let mut out = String::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                out.push_str(&prefix.as_os_str().to_string_lossy());
            }
            Component::RootDir => {
                if !out.ends_with('/') {
                    out.push('/');
                }
            }
            Component::CurDir => push_uri_path_segment(&mut out, "."),
            Component::ParentDir => push_uri_path_segment(&mut out, ".."),
            Component::Normal(seg) => push_uri_path_segment(&mut out, &seg.to_string_lossy()),
        }
    }
    out
}

/// Appends one path segment to `out`, inserting a `/` separator first
/// unless `out` is empty or already slash-terminated (e.g. right after a
/// `Component::RootDir`).
fn push_uri_path_segment(out: &mut String, seg: &str) {
    if !out.is_empty() && !out.ends_with('/') {
        out.push('/');
    }
    out.push_str(seg);
}

/// Decodes the small set of percent-escapes a real filesystem path
/// realistically contains (spaces and a few punctuation characters) -
/// not a general-purpose URI decoder, just enough for real filesystem
/// paths.
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The inverse of `percent_decode` - escapes every byte outside a plain
/// URI path's safe set (keeping `/` itself literal), so a real filesystem
/// path containing a space or other reserved character still produces a
/// syntactically valid `file://` URI.
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// One diagnostic's LSP shape. `Span`'s 1-indexed line/col become LSP's
/// 0-indexed line/character.
///
/// `identifier` maps to LSP's own `code` field, exactly what an editor's
/// "quick fix"/problem-matcher tooling expects a stable per-diagnostic-
/// kind string in. `tip`, when present, is appended to the message text -
/// LSP has no separate field for a
/// secondary hint either. `severity` comes from `level_to_severity` - see
/// `Diagnostic::level`'s own doc comment (checker.rs) for the tier
/// semantics this mirrors (low number = more certain/severe, not a
/// per-finding confidence score).
pub(crate) fn to_lsp_diagnostic(d: &Diagnostic) -> LspDiagnostic {
    let message = match d.tip_text() {
        Some(tip) => format!("{}\n💡 {tip}", d.message),
        None => d.message.clone(),
    };
    LspDiagnostic {
        range: to_lsp_range(d.span),
        severity: Some(level_to_severity(d.level)),
        code: Some(NumberOrString::String(d.identifier.to_string())),
        source: Some("phpstan-rs".to_string()),
        message,
        ..Default::default()
    }
}

/// Maps `Diagnostic::level` (0 lowest/laxest - see that field's own doc
/// comment) to an LSP severity tier. Level 0 ("definitely broken":
/// undefined symbol/arity) is the only bucket that unambiguously means
/// the code cannot be right, so it's the only `ERROR`; level 6 is the
/// only bucket this project's own doc comments single out as a
/// speculative, no-real-PHPStan-equivalent invention, so it's the only
/// `HINT`. `_` (not a literal `6`) catches any future level above 6,
/// since `level` is a plain unconstrained `u8` with no type-level upper
/// bound - degrading to the least-alarming tier is safer than panicking
/// or miscategorizing it as `ERROR`/`WARNING`.
pub(crate) fn level_to_severity(level: u8) -> DiagnosticSeverity {
    match level {
        0 => DiagnosticSeverity::ERROR,
        1..=5 => DiagnosticSeverity::WARNING,
        _ => DiagnosticSeverity::HINT,
    }
}

/// `Span`'s 1-indexed line/col becomes LSP's 0-indexed line/character -
/// shared by every diagnostic/hover/definition response this server
/// sends, so the same approximation (this project's own `Span` counts
/// UTF-8 characters; LSP wants UTF-16 code units - see this module's own
/// doc comment) is made exactly once.
pub(crate) fn to_lsp_range(span: Span) -> LspRange {
    LspRange {
        start: to_lsp_position(span.start),
        end: to_lsp_position(span.end),
    }
}

/// The single-position half of `to_lsp_range` - see its own doc comment
/// for the 1-indexed-vs-0-indexed/UTF-8-vs-UTF-16 caveats, which apply
/// identically here.
pub(crate) fn to_lsp_position(pos: Position) -> LspPosition {
    LspPosition {
        line: pos.line.saturating_sub(1),
        character: pos.col.saturating_sub(1),
    }
}

/// Whether `a` and `b` overlap, including merely touching at a point (a
/// zero-width `a` - a bare cursor, no selection - still overlaps a
/// diagnostic span it sits inside or right at the edge of). Compares
/// `Position` endpoints directly: `Position` derives `Ord`/`PartialOrd`
/// (lexicographic on line-then-character, matching field declaration
/// order) in lsp_types 0.97.0, but `Range` itself does not - there is no
/// whole-`Range` comparison to make here, only this one.
pub(crate) fn ranges_overlap(a: LspRange, b: LspRange) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// Converts a diagnostic's suggested `DiagnosticFix` into a `quickfix`
/// `CodeAction` - `None` when `d` has no fix to offer. `DiagnosticFix`
/// has no path field of its own (every fix is implicitly same-file), so
/// this takes the request's own `uri` rather than deriving one from the
/// diagnostic - correct today, forecloses a future cross-file fix
/// without a schema change.
pub(crate) fn diagnostic_fix_to_code_action(
    uri: &Uri,
    d: &Diagnostic,
    source: Option<&str>,
) -> Option<CodeActionOrCommand> {
    let fix = d.fix.as_ref()?;
    let span = match source {
        Some(source) => diagnostics::fix::resolve_span(source, fix)?,
        None if fix.within.is_none() => fix.span,
        None => return None,
    };
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: fix.description.clone(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![to_lsp_diagnostic(d)]),
        edit: Some(WorkspaceEdit {
            changes: Some(
                [(
                    uri.clone(),
                    vec![TextEdit {
                        range: to_lsp_range(span),
                        new_text: fix.replacement.clone(),
                    }],
                )]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        }),
        is_preferred: Some(true),
        ..Default::default()
    }))
}

/// The inverse of `to_lsp_range`'s own position half - a request's cursor
/// position, converted from LSP's 0-indexed/UTF-16 convention to this
/// project's own 1-indexed/UTF-8-character one. Same accepted
/// approximation, just applied in the other direction.
pub(crate) fn from_lsp_position(pos: LspPosition) -> Position {
    Position {
        line: pos.line + 1,
        col: pos.character + 1,
    }
}

pub(crate) fn publish_diagnostics_notification(
    uri: Uri,
    version: Option<i32>,
    diagnostics: &[Diagnostic],
) -> Message {
    let mut ordered: Vec<&Diagnostic> = diagnostics.iter().collect();
    ordered.sort_by(|a, b| diagnostic_order(a, b));
    Message::Notification(LspNotification::new(
        "textDocument/publishDiagnostics".to_string(),
        PublishDiagnosticsParams {
            uri,
            diagnostics: ordered.into_iter().map(to_lsp_diagnostic).collect(),
            version,
        },
    ))
}

/// A `$/progress` notification carrying one `WorkDoneProgress` value
/// (`Begin`/`Report`/`End`) for `token` - a client-supplied
/// `workDoneToken` from `initialize`, or one this server requested via
/// `window/workDoneProgress/create`.
pub(crate) fn progress_notification(token: ProgressToken, value: WorkDoneProgress) -> Message {
    Message::Notification(LspNotification::new(
        "$/progress".to_string(),
        ProgressParams {
            token,
            value: ProgressParamsValue::WorkDone(value),
        },
    ))
}

/// A `window/logMessage` notification - surfaces a recoverable failure
/// (a workspace file that failed to parse, a real transport I/O error)
/// to the editor's Output channel, not just this process's own stderr,
/// which a real editor session never shows the user by default.
pub(crate) fn log_message_notification(typ: MessageType, message: String) -> Message {
    Message::Notification(LspNotification::new(
        "window/logMessage".to_string(),
        LogMessageParams { typ, message },
    ))
}

/// Converts `$/cancelRequest`'s wire-level `NumberOrString` id into the
/// `lsp_server::RequestId` every pending-request map is keyed by -
/// infallible, since `RequestId` has a direct `From` impl for both
/// `i32` and `String`.
pub(crate) fn to_request_id(id: NumberOrString) -> RequestId {
    match id {
        NumberOrString::Number(n) => RequestId::from(n),
        NumberOrString::String(s) => RequestId::from(s),
    }
}
