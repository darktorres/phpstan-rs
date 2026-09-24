use rustc_hash::FxHashMap;
use std::path::PathBuf;

use super::*;
use crate::token::{Position, Span};

fn diag(location: &str, message: &str, line: u32, col: u32, level: u8) -> Diagnostic {
    diag_with_identifier(location, message, line, col, level, "variable.undefined")
}

fn diag_with_identifier(
    location: &str,
    message: &str,
    line: u32,
    col: u32,
    level: u8,
    identifier: &str,
) -> Diagnostic {
    Diagnostic {
        location: location.to_string(),
        message: message.to_string(),
        span: Span {
            start: Position { line, col },
            end: Position { line, col },
        },
        level,
        identifier: identifier.to_string(),
        tip: None,
        non_ignorable: false,
        metadata: FxHashMap::default(),
        fix: None,
    }
}

#[test]
fn diagnostic_line_matches_this_projects_original_text_shape() {
    let d = diag("f", "undefined variable: $x", 3, 12, 0);
    assert_eq!(
        diagnostic_line(&d),
        "3:12: f: undefined variable: $x (variable.undefined)"
    );
}

#[test]
fn render_json_includes_totals_and_per_file_messages() {
    let mut results = FxHashMap::default();
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
    assert_eq!(file["messages"][0]["identifier"], "variable.undefined");
    // tip is omitted entirely (not even `null`) when there isn't one.
    assert!(file["messages"][0].get("tip").is_none());
}

#[test]
fn render_json_omits_clean_files_but_counts_them_in_files_checked() {
    let mut results = FxHashMap::default();
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
    let results: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
    let json = render_json(&results, 0);
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["totals"]["files_checked"], 0);
    assert_eq!(parsed["totals"]["diagnostics"], 0);
    assert_eq!(parsed["files"].as_object().unwrap().len(), 0);
}

#[test]
fn render_checkstyle_produces_well_formed_xml_with_one_error_per_diagnostic() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("src/bad.php"),
        vec![
            diag("f", "undefined variable: $x", 3, 12, 0),
            diag_with_identifier(
                "f",
                "return type mismatch: expected int, got string",
                4,
                5,
                3,
                "return.type",
            ),
        ],
    );

    let xml = render_checkstyle(&results);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
    assert!(xml.contains("<checkstyle"));
    assert!(xml.contains("<file name=\"src/bad.php\">"));
    assert!(xml.contains(
        "<error line=\"3\" column=\"12\" severity=\"error\" \
         message=\"f: undefined variable: $x\" source=\"phpstan-rs.variable.undefined\"/>"
    ));
    assert!(xml.contains(
        "<error line=\"4\" column=\"5\" severity=\"error\" \
         message=\"f: return type mismatch: expected int, got string\" \
         source=\"phpstan-rs.return.type\"/>"
    ));
    assert!(xml.trim_end().ends_with("</checkstyle>"));
}

#[test]
fn render_checkstyle_escapes_special_characters_in_messages() {
    let mut results = FxHashMap::default();
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
    let mut results = FxHashMap::default();
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

#[test]
fn a_multi_line_message_stays_one_line_in_text_but_intact_in_json_and_checkstyle() {
    let d = diag("f", "value (first\nsecond\r\nthird): bad", 1, 1, 2);
    assert_eq!(
        diagnostic_line(&d),
        "1:1: f: value (first second third): bad (variable.undefined)"
    );
    let mut results = FxHashMap::default();
    results.insert(PathBuf::from("a.php"), vec![d]);
    assert!(render_json(&results, 1).contains(r#"value (first\nsecond\r\nthird): bad"#));
    assert!(render_checkstyle(&results).contains("value (first&#10;second&#13;&#10;third): bad"));
}

#[test]
fn junit_reports_each_diagnostic_as_a_failing_case_and_clean_files_as_passing() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("src/a.php"),
        vec![diag_with_identifier(
            "f",
            "Bad <thing> & more",
            3,
            5,
            0,
            "argument.type",
        )],
    );
    results.insert(PathBuf::from("src/b.php"), vec![]);
    let xml = render_junit(&results);
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<testsuite name=\"phpstan-rs\" tests=\"2\" failures=\"1\" errors=\"0\" time=\"0\">\n  \
<testcase name=\"src/a.php:3\" classname=\"phpstan-rs.argument.type\">\n    \
<failure type=\"argument.type\" message=\"Bad &lt;thing&gt; &amp; more\">f: Bad &lt;thing&gt; &amp; more</failure>\n  \
</testcase>\n  \
<testcase name=\"src/b.php\"/>\n\
</testsuite>\n"
    );
}

fn friendly_fixture(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("phpstan-rs-friendly-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.php");
    std::fs::write(
        &path,
        "<?php\n// note\nfunction f(int $x): string {\n    return $x + 1;\n}\n",
    )
    .unwrap();
    path
}

#[test]
fn friendly_frames_the_reported_line_with_its_span_underlined() {
    let path = friendly_fixture("plain");
    let mut d = diag_with_identifier("f", "Wrong return.", 4, 12, 0, "return.type");
    d.span.end = Position { line: 4, col: 18 };
    d.tip = Some("Return a string.".to_string());
    let results = FxHashMap::from_iter([(path.clone(), vec![d])]);
    let text = render_friendly(&results, false);
    let expected = format!(
        " ✖ Wrong return.\n  {}:4:12 · f · return.type\n    2 | // note\n    3 | function f(int $x): string {{\n  > 4 |     return $x + 1;\n      |            ^^^^^^\n    5 | }}\n  💡 Return a string.\n\n",
        path.display()
    );
    assert_eq!(text, expected);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn friendly_highlights_code_when_colour_is_on() {
    let path = friendly_fixture("colour");
    let results = FxHashMap::from_iter([(
        path.clone(),
        vec![diag_with_identifier(
            "f",
            "Wrong return.",
            4,
            12,
            0,
            "return.type",
        )],
    )]);
    let text = render_friendly(&results, true);
    assert!(
        text.contains("\x1b[35mreturn"),
        "keyword coloured: {text:?}"
    );
    assert!(text.contains("\x1b[36m$x"), "variable coloured: {text:?}");
    assert!(
        text.contains("\x1b[90m// note"),
        "comment coloured: {text:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
