use std::io::{self, BufRead, Write};
use std::path::{Component, Path, PathBuf, Prefix};
use std::str::FromStr;

use crossbeam_channel::Sender;
use lsp_server::{Message, Notification as LspNotification, RequestId};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic as LspDiagnostic,
    DiagnosticSeverity, LogMessageParams, MessageType, NumberOrString, Position as LspPosition,
    ProgressParams, ProgressParamsValue, ProgressToken, PublishDiagnosticsParams,
    Range as LspRange, ShowMessageParams, TextEdit, Uri, WorkDoneProgress, WorkspaceEdit,
};

use diagnostics::{Diagnostic, diagnostic_order};
use syntax::span::{LineCol, LineIndex, Range, Span, Unit};

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

/// The path a `file:` URI (RFC 8089) names on this system, or `None` when
/// it names none: another scheme, a host that is not this machine's, a
/// user or a port, a path that is not absolute, or a NUL byte.
///
/// On Unix the host is empty or `localhost`, and the path is the URI's
/// decoded bytes. On Windows a drive's path (`/C:/p`, `/c%3A/p`) gives
/// `C:\p`, and a host names a share: `file://server/share/p` gives
/// `\\server\share\p`, the hosts `?` and `.` the verbatim and device
/// namespaces. A Windows name in a URI is UTF-8.
pub(crate) fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    if !uri.scheme()?.as_str().eq_ignore_ascii_case("file") {
        return None;
    }
    let host = match uri.authority() {
        Some(authority) if authority.userinfo().is_some() || authority.port().is_some() => {
            return None;
        }
        Some(authority) => authority.host().as_str(),
        None => "",
    };
    let host = if host.eq_ignore_ascii_case("localhost") {
        Vec::new()
    } else {
        percent_decoded(host)?
    };
    let path = percent_decoded(uri.path().as_str())?;
    if host.contains(&0) || path.contains(&0) {
        return None;
    }
    file_path(&host, &path)
}

/// The path that a `file:` URI's decoded host and path name on Unix.
#[cfg(unix)]
fn file_path(host: &[u8], path: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    (host.is_empty() && path.starts_with(b"/"))
        .then(|| PathBuf::from(std::ffi::OsStr::from_bytes(path)))
}

/// The path that a `file:` URI's decoded host and path name on Windows.
#[cfg(windows)]
fn file_path(host: &[u8], path: &[u8]) -> Option<PathBuf> {
    let host = std::str::from_utf8(host).ok()?;
    let path = std::str::from_utf8(path).ok()?.strip_prefix('/')?;
    let text = if host.is_empty() {
        let drive = path.as_bytes();
        let names_a_drive = drive.len() >= 2
            && drive[0].is_ascii_alphabetic()
            && drive[1] == b':'
            && matches!(drive.get(2), None | Some(b'/'));
        if !names_a_drive {
            return None;
        }
        let mut text = path.replace('/', r"\");
        if text.len() == 2 {
            text.push('\\');
        }
        text
    } else {
        if path.split('/').next().is_none_or(str::is_empty) {
            return None;
        }
        format!(r"\\{host}\{}", path.replace('/', r"\"))
    };
    Some(PathBuf::from(text))
}

/// The `file:` URI of the absolute path `path` (RFC 8089): each name
/// percent-encoded after a `/`, on Unix from its bytes, since `\` is a name's
/// character there. On Windows a drive's path is `/C:/…` and a share's
/// `//server/share/…`, and the verbatim and device namespaces are shares of
/// the hosts `?` and `.`, as VS Code spells them. A verbatim drive or share
/// loses its prefix: resolving the path the URI names gives it back
/// (`canonical::form`).
pub(crate) fn path_to_uri(path: &Path) -> Uri {
    let mut uri = String::from("file://");
    let mut named = false;
    for component in path.components() {
        let name = match component {
            Component::Prefix(prefix) => {
                push_prefix(&mut uri, prefix.kind());
                continue;
            }
            Component::RootDir => continue,
            Component::CurDir => b".".as_slice(),
            Component::ParentDir => b"..".as_slice(),
            Component::Normal(name) => name.as_encoded_bytes(),
        };
        push_name(&mut uri, name);
        named = true;
    }
    if path.has_root() && !named {
        uri.push('/');
    }
    Uri::from_str(&uri).expect("a percent-encoded path makes a valid file: URI")
}

/// Appends a Windows prefix as a `file:` URI spells it: a drive as the first
/// name, anything else as a host and the name after it.
fn push_prefix(uri: &mut String, prefix: Prefix<'_>) {
    let (host, name) = match prefix {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
            uri.push('/');
            uri.push(char::from(drive));
            uri.push(':');
            return;
        }
        Prefix::UNC(server, name) | Prefix::VerbatimUNC(server, name) => {
            (server.as_encoded_bytes(), name)
        }
        Prefix::Verbatim(name) => (b"?".as_slice(), name),
        Prefix::DeviceNS(name) => (b".".as_slice(), name),
    };
    push_encoded(uri, host, false);
    push_name(uri, name.as_encoded_bytes());
}

/// Appends `/` and the name `name`, percent-encoded.
fn push_name(uri: &mut String, name: &[u8]) {
    uri.push('/');
    push_encoded(uri, name, true);
}

/// Appends `bytes` with every byte percent-encoded but the unreserved ones
/// and, in a name, `:`.
fn push_encoded(uri: &mut String, bytes: &[u8], in_name: bool) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in bytes {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(char::from(byte));
            }
            b':' if in_name => uri.push(':'),
            _ => {
                uri.push('%');
                uri.push(char::from(HEX[usize::from(byte >> 4)]));
                uri.push(char::from(HEX[usize::from(byte & 0xF)]));
            }
        }
    }
}

/// The bytes the percent-encoded `text` stands for, or `None` when an
/// escape is not `%` and two hex digits.
fn percent_decoded(text: &str) -> Option<Vec<u8>> {
    let mut bytes = text.bytes();
    let mut decoded = Vec::with_capacity(text.len());
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            decoded.push(byte);
            continue;
        }
        let high = char::from(bytes.next()?).to_digit(16)?;
        let low = char::from(bytes.next()?).to_digit(16)?;
        decoded.push(u8::try_from(high << 4 | low).ok()?);
    }
    Some(decoded)
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
pub(crate) fn to_lsp_diagnostic(
    d: &Diagnostic,
    index: Option<&LineIndex>,
    unit: Unit,
) -> LspDiagnostic {
    let message = match d.tip_text() {
        Some(tip) => format!("{}\n💡 {tip}", d.message),
        None => d.message.clone(),
    };
    LspDiagnostic {
        range: range_to_lsp(index, unit, d.span),
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

/// A file's lines and the unit its columns are counted in: what turns a
/// byte offset into an LSP position.
#[derive(Clone, Copy)]
pub(crate) struct Positions<'a> {
    pub(crate) index: &'a LineIndex,
    pub(crate) unit: Unit,
}

impl<'a> Positions<'a> {
    /// `index`'s columns in UTF-16 code units, the protocol's default.
    #[cfg(test)]
    pub(crate) fn utf16(index: &'a LineIndex) -> Positions<'a> {
        Positions {
            index,
            unit: Unit::Utf16,
        }
    }

    pub(crate) fn position(self, offset: u32) -> LspPosition {
        lsp_position(self.index, self.unit, offset)
    }

    pub(crate) fn range(self, span: Span) -> LspRange {
        span_to_lsp_range(self.index, self.unit, span)
    }
}

/// The LSP position of `offset` in the file whose lines `index` holds, its
/// character counted in the `unit` the client and server agreed on.
pub(crate) fn lsp_position(index: &LineIndex, unit: Unit, offset: u32) -> LspPosition {
    LspPosition {
        line: index.line(offset) - 1,
        character: index.column(offset, unit),
    }
}

/// The LSP range of the lines and columns `span` has in the file whose
/// lines `index` holds.
pub(crate) fn span_to_lsp_range(index: &LineIndex, unit: Unit, span: Span) -> LspRange {
    LspRange {
        start: lsp_position(index, unit, span.start),
        end: lsp_position(index, unit, span.end),
    }
}

/// The LSP range of a diagnostic's `range`, whose columns count
/// characters. With the file's `index` the columns are recounted in
/// `unit`; without one (the file is not loaded) the characters stand.
pub(crate) fn range_to_lsp(index: Option<&LineIndex>, unit: Unit, range: Range) -> LspRange {
    let position = |at: LineCol| match index
        .and_then(|index| index.offset(at).map(|offset| (index, offset)))
    {
        Some((index, offset)) => lsp_position(index, unit, offset),
        None => LspPosition {
            line: at.line.saturating_sub(1),
            character: at.col.saturating_sub(1),
        },
    };
    LspRange {
        start: position(range.start),
        end: position(range.end),
    }
}

/// Whether `a` and `b` overlap, including merely touching at a point (a
/// zero-width `a` - a bare cursor, no selection - still overlaps a
/// diagnostic span it sits inside or right at the edge of). Compares
/// `LineCol` endpoints directly: `LineCol` derives `Ord`/`PartialOrd`
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
    positions: Positions<'_>,
) -> Option<CodeActionOrCommand> {
    let fix = d.fix.as_ref()?;
    let span = match source {
        Some(source) => diagnostics::fix::resolve_span(source.as_bytes(), fix)?,
        None if fix.within.is_none() => fix.span,
        None => return None,
    };
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: fix.description.clone(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![to_lsp_diagnostic(
            d,
            Some(positions.index),
            positions.unit,
        )]),
        edit: Some(WorkspaceEdit {
            changes: Some(
                [(
                    uri.clone(),
                    vec![TextEdit {
                        range: positions.range(span),
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

/// The byte offset of a request's cursor `position` in the file whose
/// lines `index` holds, its character counted in `unit`.
pub(crate) fn offset_from_lsp(index: &LineIndex, unit: Unit, position: LspPosition) -> Option<u32> {
    index.offset_in(position.line, position.character, unit)
}

pub(crate) fn publish_diagnostics_notification(
    uri: Uri,
    version: Option<i32>,
    diagnostics: &[Diagnostic],
    index: Option<&LineIndex>,
    unit: Unit,
) -> Message {
    let mut ordered: Vec<&Diagnostic> = diagnostics.iter().collect();
    ordered.sort_by(|a, b| diagnostic_order(a, b));
    Message::Notification(LspNotification::new(
        "textDocument/publishDiagnostics".to_string(),
        PublishDiagnosticsParams {
            uri,
            diagnostics: ordered
                .into_iter()
                .map(|d| to_lsp_diagnostic(d, index, unit))
                .collect(),
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

/// A `window/showMessage` notification - shows the user a problem they must
/// act on, such as a configuration that could not be used as written.
pub(crate) fn show_message_notification(typ: MessageType, message: String) -> Message {
    Message::Notification(LspNotification::new(
        "window/showMessage".to_string(),
        ShowMessageParams { typ, message },
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
