//! Wiring a workspace's own `phpstan-rs.toml` (config/composer/opt-in-
//! check/ignore_errors/baseline/severity-level resolution) into LSP
//! mode, via the real `initialize`->`initialized`->`didOpen` sequence.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

const DEPRECATED_FIXTURE: &str =
    "<?php\n/** @deprecated use newFunc() instead */\nfunction oldFunc(): void {}\noldFunc();\n";

const PROMOTABLE_CTOR_FIXTURE: &str = "<?php\nclass Point {\n    public int $x;\n    public int $y;\n    public function __construct(int $x, int $y) {\n        $this->x = $x;\n        $this->y = $y;\n    }\n}\n";

fn diagnostics_for(messages: &[Value], uri: &str) -> Vec<Value> {
    messages
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .flat_map(|m| m["params"]["diagnostics"].as_array().unwrap().clone())
        .collect()
}

#[test]
fn no_config_file_leaves_opt_in_checks_off() {
    let dir = TempDir::new("no_config");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": DEPRECATED_FIXTURE}}),
        )],
    );

    let diags = diagnostics_for(&messages, &uri);
    assert!(
        !diags.iter().any(|d| d["code"] == "function.deprecated"),
        "got: {diags:?}"
    );
}

#[test]
fn workspace_root_config_file_turns_on_an_opt_in_check() {
    let dir = TempDir::new("config_turns_on_check");
    dir.write("phpstan-rs.toml", "check_deprecated_usage = true\n");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": DEPRECATED_FIXTURE}}),
        )],
    );

    let diags = diagnostics_for(&messages, &uri);
    assert!(
        diags.iter().any(|d| d["code"] == "function.deprecated"),
        "got: {diags:?}"
    );
}

#[test]
fn workspace_root_config_wires_php_version_together_with_a_gated_check() {
    let dir_80 = TempDir::new("promo_php80");
    dir_80.write(
        "phpstan-rs.toml",
        "check_promoted_property_suggestions = true\nphp_version = \"8.0\"\n",
    );
    let path_80 = dir_80.write("a.php", PROMOTABLE_CTOR_FIXTURE);
    let uri_80 = path_to_uri(&path_80).to_string();
    let messages_80 = run(
        &dir_80,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri_80, "languageId": "php", "version": 1, "text": PROMOTABLE_CTOR_FIXTURE}}),
        )],
    );
    let diags_80 = diagnostics_for(&messages_80, &uri_80);
    assert!(
        diags_80
            .iter()
            .any(|d| d["code"] == "constructor.promotionCandidate"),
        "got: {diags_80:?}"
    );

    let dir_74 = TempDir::new("promo_php74");
    dir_74.write(
        "phpstan-rs.toml",
        "check_promoted_property_suggestions = true\nphp_version = \"7.4\"\n",
    );
    let path_74 = dir_74.write("a.php", PROMOTABLE_CTOR_FIXTURE);
    let uri_74 = path_to_uri(&path_74).to_string();
    let messages_74 = run(
        &dir_74,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri_74, "languageId": "php", "version": 1, "text": PROMOTABLE_CTOR_FIXTURE}}),
        )],
    );
    let diags_74 = diagnostics_for(&messages_74, &uri_74);
    assert!(
        !diags_74
            .iter()
            .any(|d| d["code"] == "constructor.promotionCandidate"),
        "got: {diags_74:?}"
    );
}

#[test]
fn a_malformed_config_file_degrades_to_defaults_instead_of_killing_the_session() {
    let dir = TempDir::new("malformed_config");
    dir.write("phpstan-rs.toml", "this is not valid toml {{{");

    let messages = run(&dir, &[]);

    let response = messages
        .iter()
        .find(|m| m["id"] == json!(1))
        .unwrap_or_else(|| panic!("expected the initialize response, got: {messages:?}"));
    assert!(response["result"]["capabilities"].is_object());
}

#[test]
fn vendor_context_from_a_real_composer_project_is_loaded_for_navigation() {
    let dir = TempDir::new("vendor_context_hover");
    dir.write(
        "composer.json",
        r#"{"require": {"acme/lib": "1.0.0"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
    );
    dir.write(
        "vendor/composer/autoload_classmap.php",
        "<?php\nreturn ['Acme\\\\Lib\\\\Foo' => __DIR__ . '/../acme/lib/Foo.php'];\n",
    );
    dir.write(
        "vendor/acme/lib/Foo.php",
        "<?php\nnamespace Acme\\Lib;\nclass Foo {}\n",
    );
    let bar_src = "<?php\nuse Acme\\Lib\\Foo;\nclass Bar extends Foo {}\n";
    let bar_path = dir.write("src/Bar.php", bar_src);
    let bar_uri = path_to_uri(&bar_path).to_string();
    let pos = lsp_pos(bar_src, "extends Foo");
    let pos = json!({"line": pos["line"], "character": pos["character"].as_u64().unwrap() + "extends ".len() as u64});

    let messages = run(&dir, &[hover_request(2, &bar_uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        result["contents"], "class Acme\\Lib\\Foo",
        "got: {result:?}"
    );
}

#[test]
fn vendor_context_reachable_via_composer_and_also_under_the_workspace_root_is_not_double_loaded() {
    let dir = TempDir::new("vendor_no_double_load");
    dir.write(
        "phpstan-rs.toml",
        "check_duplicate_class_declarations = true\n",
    );
    dir.write(
        "composer.json",
        r#"{"require": {"acme/lib": "1.0.0"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
    );
    dir.write(
        "vendor/composer/autoload_classmap.php",
        "<?php\nreturn ['Acme\\\\Lib\\\\Foo' => __DIR__ . '/../acme/lib/Foo.php'];\n",
    );
    let foo_src = "<?php\nnamespace Acme\\Lib;\nclass Foo {}\n";
    let foo_path = dir.write("vendor/acme/lib/Foo.php", foo_src);
    let foo_uri = path_to_uri(&foo_path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": foo_uri, "languageId": "php", "version": 1, "text": foo_src}}),
        )],
    );

    let diags = diagnostics_for(&messages, &foo_uri);
    assert!(
        !diags.iter().any(|d| d["code"] == "class.duplicate"),
        "got: {diags:?}"
    );
}

#[test]
fn analyzed_first_party_declaration_wins_over_a_same_named_vendor_class() {
    let dir = TempDir::new("analyzed_wins_collision");
    dir.write(
        "composer.json",
        r#"{"require": {"acme/lib": "1.0.0"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
    );
    dir.write(
        "vendor/composer/autoload_classmap.php",
        "<?php\nreturn ['Foo' => __DIR__ . '/../acme/lib/Foo.php'];\n",
    );
    dir.write(
        "vendor/acme/lib/Foo.php",
        "<?php\nclass Foo { public function real(): void {} }\n",
    );
    dir.write(
        "src/Foo.php",
        "<?php\nclass Foo { public function override(): void {} }\n",
    );
    let bar_src = "<?php\nclass Bar extends Foo {\n    public function useIt() {\n        $this->override();\n        $this->real();\n    }\n}\n";
    let bar_path = dir.write("src/Bar.php", bar_src);
    let bar_uri = path_to_uri(&bar_path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": bar_uri, "languageId": "php", "version": 1, "text": bar_src}}),
        )],
    );

    let diags = diagnostics_for(&messages, &bar_uri);
    assert!(
        diags
            .iter()
            .any(|d| d["message"].as_str().unwrap_or("").contains("real")),
        "expected an undefined-method diagnostic for ->real(), got: {diags:?}"
    );
    assert!(
        !diags
            .iter()
            .any(|d| d["message"].as_str().unwrap_or("").contains("override")),
        "->override() must resolve (the first-party Foo won outright), got: {diags:?}"
    );
}

#[test]
fn ignore_errors_entries_from_config_suppress_matching_diagnostics_in_the_editor() {
    let dir = TempDir::new("ignore_errors_config");
    dir.write(
        "phpstan-rs.toml",
        "check_deprecated_usage = true\n\n[[ignore_errors]]\nidentifier = \"function.deprecated\"\n",
    );
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": DEPRECATED_FIXTURE}}),
        )],
    );

    let diags = diagnostics_for(&messages, &uri);
    assert!(
        !diags.iter().any(|d| d["code"] == "function.deprecated"),
        "got: {diags:?}"
    );
}

#[test]
fn a_configured_level_cutoff_hides_higher_severity_diagnostics() {
    let dir = TempDir::new("level_cutoff");
    dir.write(
        "phpstan-rs.toml",
        "check_promoted_property_suggestions = true\nphp_version = \"8.0\"\nlevel = 0\n",
    );
    let path = dir.write("a.php", PROMOTABLE_CTOR_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": PROMOTABLE_CTOR_FIXTURE}}),
        )],
    );

    let diags = diagnostics_for(&messages, &uri);
    assert!(
        !diags
            .iter()
            .any(|d| d["code"] == "constructor.promotionCandidate"),
        "level=0 must hide a level-5 diagnostic, got: {diags:?}"
    );
}

#[test]
fn baseline_entries_from_a_root_relative_baseline_file_suppress_matching_diagnostics() {
    // Baseline entries key on the diagnostic's exact (file, location,
    // message) triple - captured here from a real baseline-less run
    // rather than guessed, since `to_lsp_diagnostic` joins
    // `location`/`message` as `"{location}: {message}"` on the wire.
    let control_dir = TempDir::new("baseline_config_control");
    control_dir.write("phpstan-rs.toml", "check_deprecated_usage = true\n");
    let control_path = control_dir.write("a.php", DEPRECATED_FIXTURE);
    let control_uri = path_to_uri(&control_path).to_string();
    let control_messages = run(
        &control_dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": control_uri, "languageId": "php", "version": 1, "text": DEPRECATED_FIXTURE}}),
        )],
    );
    let control_diags = diagnostics_for(&control_messages, &control_uri);
    let message = control_diags
        .iter()
        .find(|d| d["code"] == "function.deprecated")
        .unwrap_or_else(|| panic!("expected function.deprecated in {control_diags:?}"))["message"]
        .as_str()
        .unwrap()
        .to_string();

    let dir = TempDir::new("baseline_config");
    dir.write(
        "phpstan-rs.toml",
        "check_deprecated_usage = true\nbaseline = \"baseline.json\"\n",
    );
    dir.write(
        "baseline.json",
        &serde_json::to_string(
            &json!({"version": 2, "entries": [{"file": "a.php", "message": message}]}),
        )
        .unwrap(),
    );
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": DEPRECATED_FIXTURE}}),
        )],
    );

    let diags = diagnostics_for(&messages, &uri);
    assert!(
        !diags.iter().any(|d| d["code"] == "function.deprecated"),
        "got: {diags:?}"
    );
    assert!(
        !messages.iter().any(|m| m["method"] == "window/logMessage"
            && m["params"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("baseline")),
        "handle_open_or_change must never surface baseline::filter's stale-entry report: {messages:?}"
    );
}
