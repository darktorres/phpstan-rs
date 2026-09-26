//! CI-friendly diagnostic output formats for `check-project`.
//!
//! This project's original `path:line:col: location: message` text format
//! stays the default and is unaffected by anything here - it's what a
//! human reads at a terminal. The formats below exist so a CI pipeline can
//! consume `check-project`'s output as structured data instead of grepping
//! plain text: `json` (the most universally consumable - every CI system
//! can parse JSON with no extra tooling) and `checkstyle` (the XML shape
//! understood natively by Jenkins' Checkstyle plugin, GitLab's Code
//! Quality XML->JSON converters, and most CI dashboards that don't
//! specifically integrate PHPStan itself).
//!
//! `Diagnostic::location` is a caller-supplied *context* label (the
//! enclosing function/method/class a finding was reported against - e.g.
//! `"f"` or `"Shape::area"`, sometimes the catch-all `"<main>"` for
//! top-level script code), not a rule ID, so it's surfaced below under
//! that same honest name rather than mislabeled "identifier"/"rule".
//! `Diagnostic`'s stable per-check `identifier` field is also surfaced:
//! `json` as its own `identifier` field per message, `checkstyle` as the
//! `source` attribute (the field most Checkstyle-consuming tools treat as
//! a rule name for grouping/suppression), with `location` folded into
//! the message text there instead so neither piece of context is lost.

use rustc_hash::FxHashMap;
use std::collections::BTreeMap;
use std::path::PathBuf;

use diagnostics::Diagnostic;
use serde::Serialize;

mod codeframe;
pub use codeframe::render_friendly;

/// Selects how `check-project` renders its diagnostics. `Text` (the
/// default) is this project's original human-readable format; `Json`,
/// `Checkstyle` and `Junit` are CI-consumable additions, and `Friendly` a
/// richer terminal rendering. A plain
/// `#[derive(clap::ValueEnum)]` rather than hand-rolled `FromStr`/error
/// message - clap already renders a helpful "invalid value 'x' for
/// '--format <FORMAT>' [possible values: text, json, checkstyle, junit, friendly]" on a bad
/// value for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Checkstyle,
    /// JUnit XML: one test case per diagnostic, one passing case per
    /// clean file.
    Junit,
    /// Each diagnostic with a highlighted frame of the source around it.
    Friendly,
}

/// One diagnostic in the text format, *without* the leading file path -
/// `line:column: message (identifier)`, shared by every printing path
/// (`check-project`'s own `Text` format, `incremental-bench`, and `demo`).
/// `check-project` (the only one of the three that reports on more than
/// one file) prepends the path itself. Always exactly one line: a message
/// that itself spans lines (a `phpDoc.parseError` quoting a wrapped tag's
/// value, the same text real PHPStan reports) has each line break shown as
/// a space here, so one diagnostic stays one line for grep and editor
/// problem matchers. The JSON and checkstyle formats keep it intact.
pub fn diagnostic_line(d: &Diagnostic) -> String {
    format!(
        "{}:{}: {} ({})",
        d.span.start.line,
        d.span.start.col,
        single_line(&d.message),
        d.identifier
    )
}

/// `text` with every `\r\n`/`\n`/`\r` line break replaced by one space.
fn single_line(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\n', '\r']) {
        std::borrow::Cow::Owned(text.replace("\r\n", " ").replace(['\n', '\r'], " "))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

#[derive(Serialize)]
struct JsonMessage<'a> {
    message: &'a str,
    line: u32,
    column: u32,
    level: u8,
    identifier: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tip: Option<String>,
}

#[derive(Serialize)]
struct JsonFile<'a> {
    diagnostics: usize,
    messages: Vec<JsonMessage<'a>>,
}

#[derive(Serialize)]
struct JsonTotals {
    files_checked: usize,
    diagnostics: usize,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    totals: JsonTotals,
    files: BTreeMap<String, JsonFile<'a>>,
}

/// Renders `results` (already level-/baseline-filtered) as JSON: a
/// `totals` object plus a `files` map keyed by display path, each holding
/// a `messages` array - the shape most CI JSON consumers (a generic
/// "eslint-style" reporter, a small jq/Python script in a pipeline) expect
/// out of the box. `files_checked` is passed in by the caller rather than
/// computed from `results.len()`, since `results` (see `run_check_project`
/// and `baseline::filter`'s own doc comment) can hold an entry with zero
/// diagnostics for every analyzed file, including ones that failed to
/// parse and were never really "checked" - the same distinction the text
/// format's own summary line already makes.
///
/// A file with zero diagnostics is omitted from `files` entirely (nothing
/// to report, and an empty `messages: []` entry per clean file would just
/// be noise for a CI consumer skimming this for failures) - `totals.
/// files_checked` remains the one place the full checked-file count is
/// recorded.
pub fn render_json(results: &FxHashMap<PathBuf, Vec<Diagnostic>>, files_checked: usize) -> String {
    let mut paths: Vec<&PathBuf> = results.keys().collect();
    paths.sort();

    let mut total_diagnostics = 0usize;
    let mut files = BTreeMap::new();
    for path in paths {
        let diagnostics = &results[path];
        if diagnostics.is_empty() {
            continue;
        }
        total_diagnostics += diagnostics.len();
        let messages = diagnostics
            .iter()
            .map(|d| JsonMessage {
                message: &d.message,
                line: d.span.start.line,
                column: d.span.start.col,
                level: d.level,
                identifier: &d.identifier,
                tip: d.tip_text(),
            })
            .collect();
        files.insert(
            path.display().to_string(),
            JsonFile {
                diagnostics: diagnostics.len(),
                messages,
            },
        );
    }

    let report = JsonReport {
        totals: JsonTotals {
            files_checked,
            diagnostics: total_diagnostics,
        },
        files,
    };
    serde_json::to_string_pretty(&report).expect("JsonReport is always representable as JSON")
}

/// Renders `results` as a Checkstyle-XML report: one `<file>` per analyzed
/// file with an `<error>` per diagnostic. Every diagnostic is emitted as
/// `severity="error"` - Checkstyle's schema has no native concept of this
/// project's own 0-6 `level` scale, so there's no non-arbitrary mapping to
/// pick between (real PHPStan's own Checkstyle-shaped formatter makes the
/// same simplification: every finding is an "error"). `source` carries
/// this diagnostic's real, stable `identifier` prefixed with
/// `phpstan-rs.`, mirroring the dotted-class-name convention most
/// Checkstyle producers use for `source` - the field most Checkstyle-
/// consuming tools group/suppress by, so a real rule identifier belongs
/// there, not the `location` context label. `location` is folded into the
/// message text instead so that breadcrumb isn't lost.
pub fn render_checkstyle(results: &FxHashMap<PathBuf, Vec<Diagnostic>>) -> String {
    let mut paths: Vec<&PathBuf> = results.keys().filter(|p| !results[*p].is_empty()).collect();
    paths.sort();

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<checkstyle version=\"1.0.0\">\n");
    for path in paths {
        out.push_str(&format!(
            "  <file name=\"{}\">\n",
            xml_escape(&path.display().to_string())
        ));
        for d in &results[path] {
            out.push_str(&format!(
                "    <error line=\"{}\" column=\"{}\" severity=\"error\" message=\"{}\" source=\"phpstan-rs.{}\"/>\n",
                d.span.start.line,
                d.span.start.col,
                xml_escape(&d.message),
                xml_escape(&d.identifier),
            ));
        }
        out.push_str("  </file>\n");
    }
    out.push_str("</checkstyle>\n");
    out
}

/// Renders `results` as a JUnit XML report: a `<testsuite>` whose test
/// cases are every diagnostic (a `<failure>`, named `path:line` and
/// classed by identifier) plus one passing case per file without any, so
/// CI test-report views show both.
pub fn render_junit(results: &FxHashMap<PathBuf, Vec<Diagnostic>>) -> String {
    let mut paths: Vec<&PathBuf> = results.keys().collect();
    paths.sort();
    let failures: usize = results.values().map(Vec::len).sum();
    let clean = results.values().filter(|d| d.is_empty()).count();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<testsuite name=\"phpstan-rs\" tests=\"{}\" failures=\"{failures}\" errors=\"0\" time=\"0\">\n",
        failures + clean
    ));
    for path in paths {
        let display = xml_escape(&path.display().to_string());
        let diagnostics = &results[path];
        if diagnostics.is_empty() {
            out.push_str(&format!("  <testcase name=\"{display}\"/>\n"));
            continue;
        }
        for d in diagnostics {
            out.push_str(&format!(
                "  <testcase name=\"{display}:{}\" classname=\"phpstan-rs.{}\">\n",
                d.span.start.line,
                xml_escape(&d.identifier),
            ));
            let mut body = d.message.clone();
            if let Some(tip) = d.tip_text() {
                body.push_str(&format!("\n{tip}"));
            }
            out.push_str(&format!(
                "    <failure type=\"{}\" message=\"{}\">{}</failure>\n",
                xml_escape(&d.identifier),
                xml_escape(&d.message),
                xml_escape(&body),
            ));
            out.push_str("  </testcase>\n");
        }
    }
    out.push_str("</testsuite>\n");
    out
}

/// Escapes the five characters XML requires escaped in attribute values,
/// plus line breaks and tabs, which an XML parser would otherwise
/// normalize to spaces inside an attribute value (XML 1.0 §3.3.3).
/// Hand-rolled rather than a new dependency - `serde`/`serde_json` (used
/// for the `json` format above and already a real dependency via the
/// baseline/LSP work) has no XML counterpart in this project, and five
/// characters don't justify pulling one in just for this.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
