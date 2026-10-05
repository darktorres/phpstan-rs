//! The server's findings are the analysis's: a file that does not parse is
//! reported where it fails, and the whole-program checks run at load and
//! again on a save.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

fn did_open(uri: &str, text: &str) -> Value {
    notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": text}}),
    )
}

fn did_change(uri: &str, version: i64, text: &str) -> Value {
    notification(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}),
    )
}

fn did_save(uri: &str) -> Value {
    notification(
        "textDocument/didSave",
        json!({"textDocument": {"uri": uri}}),
    )
}

/// The codes of each publish for `uri`, in order.
fn published_codes(messages: &[Value], uri: &str) -> Vec<Vec<String>> {
    messages
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .map(|m| {
            m["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["code"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .collect()
}

const DEAD_CODE_CONFIG: &str = "[rules]\ncheck_dead_code = true\n";

#[test]
fn a_workspace_file_that_does_not_parse_is_published_with_its_parse_error() {
    let dir = TempDir::new("one_analysis_parse_error_at_load");
    let path = dir.write("broken.php", "<?php class {");
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[]);

    assert_eq!(
        published_codes(&messages, &uri),
        vec![vec!["parse.error".to_string()]]
    );
}

#[test]
fn an_edit_that_does_not_parse_is_published_with_its_parse_error_until_it_parses_again() {
    let dir = TempDir::new("one_analysis_parse_error_on_edit");
    let source = "<?php\nfunction f(): void {}\n";
    let path = dir.write("a.php", source);
    let uri = path_to_uri(&path).to_string();

    let messages = run_across_debounce(
        &dir,
        &[did_open(&uri, source), did_change(&uri, 2, "<?php class {")],
        &[did_change(&uri, 3, source)],
    );

    let published = published_codes(&messages, &uri);
    assert_eq!(
        published,
        vec![vec![], vec!["parse.error".to_string()], vec![]],
        "got: {messages:?}"
    );
}

#[test]
fn the_whole_program_checks_are_published_at_load_and_run_again_on_a_save() {
    let dir = TempDir::new("one_analysis_whole_program_on_save");
    dir.write("phpstan-rs.toml", DEAD_CODE_CONFIG);
    let a = dir.write("a.php", "<?php\nfunction helper(): void {}\n");
    let b_source = "<?php\n";
    let b = dir.write("b.php", b_source);
    let (a_uri, b_uri) = (path_to_uri(&a).to_string(), path_to_uri(&b).to_string());

    let messages = run(
        &dir,
        &[
            did_open(&b_uri, b_source),
            did_change(&b_uri, 2, "<?php\nhelper();\n"),
            did_save(&b_uri),
        ],
    );

    let published = published_codes(&messages, &a_uri);
    assert_eq!(
        published.first(),
        Some(&vec!["function.unused".to_string()]),
        "the load publishes what the whole-program checks find, got: {messages:?}"
    );
    assert_eq!(
        published.last(),
        Some(&vec![]),
        "the save runs them again and clears what the edit resolved, got: {messages:?}"
    );
}

#[test]
fn an_edit_alone_leaves_the_whole_program_findings_as_they_were() {
    let dir = TempDir::new("one_analysis_whole_program_not_on_edit");
    dir.write("phpstan-rs.toml", DEAD_CODE_CONFIG);
    let a = dir.write("a.php", "<?php\nfunction helper(): void {}\n");
    let b_source = "<?php\n";
    let b = dir.write("b.php", b_source);
    let (a_uri, b_uri) = (path_to_uri(&a).to_string(), path_to_uri(&b).to_string());

    let messages = run(
        &dir,
        &[
            did_open(&b_uri, b_source),
            did_change(&b_uri, 2, "<?php\nhelper();\n"),
        ],
    );

    let published = published_codes(&messages, &a_uri);
    assert_eq!(
        published.last(),
        Some(&vec!["function.unused".to_string()]),
        "got: {messages:?}"
    );
}

#[test]
fn a_changed_configuration_publishes_the_whole_program_findings_it_turns_on() {
    let dir = TempDir::new("one_analysis_whole_program_on_reload");
    let path = dir.write("a.php", "<?php\nfunction helper(): void {}\n");
    dir.write("dead-code.toml", DEAD_CODE_CONFIG);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[did_change_configuration(
            json!({"phpstan-rs": {"enable": true, "configPath": "dead-code.toml"}}),
        )],
    );

    assert_eq!(
        published_codes(&messages, &uri),
        vec![vec!["function.unused".to_string()]],
        "got: {messages:?}"
    );
}
