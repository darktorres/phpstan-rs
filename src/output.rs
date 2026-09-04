//! Phase 16: CI-friendly diagnostic output formats for `check-project`.
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
//! Neither format claims a stable per-check rule identifier - `checker::
//! Diagnostic` doesn't have one yet (that's Phase 47's job; see this
//! phase's own roadmap entry). `Diagnostic::location` is a caller-supplied
//! *context* label (the enclosing function/method/class a finding was
//! reported against - e.g. `"f"` or `"Shape::area"`, sometimes the
//! catch-all `"<main>"` for top-level script code), not a rule ID, so it's
//! surfaced below under that same honest name rather than mislabeled
//! "identifier"/"rule"/"source".

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::Serialize;

use crate::checker::Diagnostic;

/// Selects how `check-project` renders its diagnostics. `Text` (the
/// default) is this project's original human-readable format; `Json` and
/// `Checkstyle` are Phase 16's CI-consumable additions. A plain
/// `#[derive(clap::ValueEnum)]` rather than hand-rolled `FromStr`/error
/// message - clap already renders a helpful "invalid value 'x' for
/// '--format <FORMAT>' [possible values: text, json, checkstyle]" on a bad
/// value for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Checkstyle,
}

/// One diagnostic in this project's original text format, *without* the
/// leading file path - shared by every printing path (`check-project`'s
/// own `Text` format, `incremental-bench`, and `demo`) so the three don't
/// drift out of sync as independently hand-written `println!`s. Matches
/// what all three already printed before this module existed.
/// `check-project` (the only one of the three that reports on more than
/// one file) prepends the path itself.
pub fn diagnostic_line(d: &Diagnostic) -> String {
    format!(
        "{}:{}: {}: {}",
        d.span.start.line, d.span.start.col, d.location, d.message
    )
}

#[derive(Serialize)]
struct JsonMessage<'a> {
    message: &'a str,
    line: u32,
    column: u32,
    level: u8,
    location: &'a str,
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
pub fn render_json(results: &HashMap<PathBuf, Vec<Diagnostic>>, files_checked: usize) -> String {
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
                location: &d.location,
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
/// this project's own `location` context label (see this module's own
/// doc comment - not a rule ID) prefixed with `phpstan-rs.`, mirroring the
/// dotted-class-name convention most Checkstyle producers use for
/// `source`, so a human reading the XML still gets *some* breadcrumb of
/// which function/method/class a finding came from.
pub fn render_checkstyle(results: &HashMap<PathBuf, Vec<Diagnostic>>) -> String {
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
                xml_escape(&d.location),
            ));
        }
        out.push_str("  </file>\n");
    }
    out.push_str("</checkstyle>\n");
    out
}

/// Escapes the five characters XML requires escaped in attribute values.
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
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
