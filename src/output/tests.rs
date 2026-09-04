use std::collections::HashMap;
use std::path::PathBuf;

use super::*;
use crate::token::{Position, Span};

fn diag(location: &str, message: &str, line: u32, col: u32, level: u8) -> Diagnostic {
    Diagnostic {
        location: location.to_string(),
        message: message.to_string(),
        span: Span {
            start: Position { line, col },
            end: Position { line, col },
        },
        level,
    }
}

#[test]
fn diagnostic_line_matches_this_projects_original_text_shape() {
    let d = diag("f", "undefined variable: $x", 3, 12, 0);
    assert_eq!(diagnostic_line(&d), "3:12: f: undefined variable: $x");
}

#[test]
fn render_json_includes_totals_and_per_file_messages() {
    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("src/bad.php"),
        vec![diag("f", "undefined variable: $x", 3, 12, 0)],
    );

    let json = render_json(&results, 1);
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

    assert_eq!(parsed["totals"]["files_checked"], 1);
    assert_eq!(parsed["totals"]["diagnostics"], 1);
    let file = &parsed["files"]["src/bad.php"];
    assert_eq!(file["diagnostics"], 1);
    assert_eq!(file["messages"][0]["message"], "undefined variable: $x");
    assert_eq!(file["messages"][0]["line"], 3);
    assert_eq!(file["messages"][0]["column"], 12);
    assert_eq!(file["messages"][0]["level"], 0);
    assert_eq!(file["messages"][0]["location"], "f");
}

#[test]
fn render_json_omits_clean_files_but_counts_them_in_files_checked() {
    let mut results = HashMap::new();
    results.insert(PathBuf::from("src/clean.php"), vec![]);
    results.insert(
        PathBuf::from("src/bad.php"),
        vec![diag("f", "undefined variable: $x", 1, 1, 0)],
    );

    let json = render_json(&results, 2);
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

    // A clean file still counts toward files_checked...
    assert_eq!(parsed["totals"]["files_checked"], 2);
    assert_eq!(parsed["totals"]["diagnostics"], 1);
    // ...but never gets an entry of its own in `files` - only the file
    // that actually has something to report does.
    let files = parsed["files"].as_object().unwrap();
    assert_eq!(files.len(), 1);
    assert!(files.contains_key("src/bad.php"));
    assert!(!files.contains_key("src/clean.php"));
}

#[test]
fn render_json_report_with_zero_diagnostics_is_still_valid_and_empty() {
    let results: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
    let json = render_json(&results, 0);
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["totals"]["files_checked"], 0);
    assert_eq!(parsed["totals"]["diagnostics"], 0);
    assert_eq!(parsed["files"].as_object().unwrap().len(), 0);
}

#[test]
fn render_checkstyle_produces_well_formed_xml_with_one_error_per_diagnostic() {
    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("src/bad.php"),
        vec![
            diag("f", "undefined variable: $x", 3, 12, 0),
            diag(
                "f",
                "return type mismatch: expected int, got string",
                4,
                5,
                3,
            ),
        ],
    );

    let xml = render_checkstyle(&results);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
    assert!(xml.contains("<checkstyle"));
    assert!(xml.contains("<file name=\"src/bad.php\">"));
    assert!(xml.contains(
        "<error line=\"3\" column=\"12\" severity=\"error\" \
         message=\"undefined variable: $x\" source=\"phpstan-rs.f\"/>"
    ));
    assert!(xml.contains(
        "<error line=\"4\" column=\"5\" severity=\"error\" \
         message=\"return type mismatch: expected int, got string\" source=\"phpstan-rs.f\"/>"
    ));
    assert!(xml.trim_end().ends_with("</checkstyle>"));
}

#[test]
fn render_checkstyle_escapes_special_characters_in_messages() {
    let mut results = HashMap::new();
    results.insert(
        PathBuf::from("src/bad.php"),
        vec![diag(
            "f",
            r#"expects array<int> but got "foo" & 'bar' <baz>"#,
            1,
            1,
            0,
        )],
    );

    let xml = render_checkstyle(&results);
    assert!(!xml.contains("\"foo\" & 'bar' <baz>"));
    assert!(xml.contains("&quot;foo&quot; &amp; &apos;bar&apos; &lt;baz&gt;"));
}

#[test]
fn render_checkstyle_omits_files_with_zero_diagnostics() {
    let mut results = HashMap::new();
    results.insert(PathBuf::from("src/clean.php"), vec![]);

    let xml = render_checkstyle(&results);
    assert!(!xml.contains("<file"));
    assert!(xml.contains("<checkstyle"));
    assert!(xml.contains("</checkstyle>"));
}

#[test]
fn output_format_default_is_text() {
    assert_eq!(OutputFormat::default(), OutputFormat::Text);
}
