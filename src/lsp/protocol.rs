use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_server::{Message, Notification as LspNotification};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticSeverity, NumberOrString, Position as LspPosition,
    PublishDiagnosticsParams, Range as LspRange, Uri,
};

use crate::checker::Diagnostic;
use crate::token::{Position, Span};

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

/// `file://`-scheme only - this server has no use for any other URI
/// scheme a client might send. `Uri::path()` is still percent-encoded
/// (fluent_uri, `lsp_types::Uri`'s own backing type, never decodes it
/// implicitly); the small `percent_decode` helper below covers the
/// common escapes a real filesystem path needs.
pub(crate) fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    if uri.scheme()?.as_str() != "file" {
        return None;
    }
    Some(PathBuf::from(percent_decode(uri.path().as_str())))
}

pub(crate) fn path_to_uri(path: &Path) -> Uri {
    let encoded = percent_encode_path(&path.display().to_string());
    Uri::from_str(&format!("file://{encoded}"))
        .expect("a percent-encoded absolute filesystem path is always a valid file:// URI")
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
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// One diagnostic's LSP shape. `location` (a symbolic label like
/// `"ClassName::methodName"`, not a position) is folded into the message
/// text instead of dropped - `lsp_types::Diagnostic` has no field for
/// it. `Span`'s 1-indexed line/col become LSP's 0-indexed line/character.
///
/// Phase 47: `identifier` now has a real home - LSP's own `code` field,
/// exactly what an editor's "quick fix"/problem-matcher tooling expects a
/// stable per-diagnostic-kind string in. `tip`, when present, is appended
/// to the message text the same way `location` already is - LSP has no
/// separate field for a secondary hint either.
pub(crate) fn to_lsp_diagnostic(d: &Diagnostic) -> LspDiagnostic {
    let message = match &d.tip {
        Some(tip) => format!("{}: {}\n💡 {tip}", d.location, d.message),
        None => format!("{}: {}", d.location, d.message),
    };
    LspDiagnostic {
        range: to_lsp_range(d.span),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(d.identifier.clone())),
        source: Some("phpstan-rs".to_string()),
        message,
        ..Default::default()
    }
}

/// `Span`'s 1-indexed line/col becomes LSP's 0-indexed line/character -
/// shared by every diagnostic/hover/definition response this server
/// sends, so the same approximation (this project's own `Span` counts
/// UTF-8 characters; LSP wants UTF-16 code units - see this module's own
/// doc comment) is made exactly once.
pub(crate) fn to_lsp_range(span: Span) -> LspRange {
    LspRange {
        start: LspPosition {
            line: span.start.line.saturating_sub(1),
            character: span.start.col.saturating_sub(1),
        },
        end: LspPosition {
            line: span.end.line.saturating_sub(1),
            character: span.end.col.saturating_sub(1),
        },
    }
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

pub(crate) fn publish_diagnostics_notification(uri: Uri, diagnostics: &[Diagnostic]) -> Message {
    Message::Notification(LspNotification::new(
        "textDocument/publishDiagnostics".to_string(),
        PublishDiagnosticsParams {
            uri,
            diagnostics: diagnostics.iter().map(to_lsp_diagnostic).collect(),
            version: None,
        },
    ))
}
