//! Filtering the workspace scan (vendor/.git/node_modules excluded from
//! the analyzed walk by name, or a configured `phpstan-rs.toml` `paths`
//! allowlist scanned instead of the whole root) and the analyzed/context
//! distinction that backs it, including the `didOpen`-of-a-never-
//! discovered-vendor-file regression this item's own Step 4 fix closes.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::json;

#[test]
fn vendor_directory_is_excluded_from_the_analyzed_walk() {
    let dir = TempDir::new("vendor_excluded");
    dir.write(
        "src/App.php",
        "<?php class App { public function ownMethod(): int { return 1; } }",
    );
    dir.write(
        "vendor/acme/App.php",
        "<?php class App { public function ghostMethod(): int { return 1; } }",
    );
    let use_src = "<?php function use_it(App $a) { $a->ownMethod(); }";
    let use_path = dir.write("src/Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "ownMethod");

    let messages = run(&dir, &[hover_request(2, &use_uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        result["contents"], "App::ownMethod(): int",
        "got: {result:?}"
    );
}

#[test]
fn dot_git_and_node_modules_are_also_excluded() {
    let dir = TempDir::new("git_and_node_modules_excluded");
    dir.write(
        "src/App.php",
        "<?php class App { public function ownMethod(): int { return 1; } }",
    );
    dir.write(
        ".git/hooks/App.php",
        "<?php class App { public function ghostMethod(): int { return 1; } }",
    );
    dir.write(
        "node_modules/pkg/App.php",
        "<?php class App { public function otherGhostMethod(): int { return 1; } }",
    );
    let use_src = "<?php function use_it(App $a) { $a->ownMethod(); }";
    let use_path = dir.write("src/Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "ownMethod");

    let messages = run(&dir, &[hover_request(2, &use_uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        result["contents"], "App::ownMethod(): int",
        "got: {result:?}"
    );
}

#[test]
fn composer_vendor_context_is_still_loaded_for_type_resolution() {
    let dir = TempDir::new("vendor_context_still_loaded");
    dir.write("composer.json", "{}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        "<?php\nreturn ['Acme\\\\VendorOnly' => __DIR__ . '/../acme/VendorOnly.php'];\n",
    );
    dir.write(
        "vendor/acme/VendorOnly.php",
        "<?php\nnamespace Acme;\nclass VendorOnly { public function vendorOnly(): int { return 1; } }\n",
    );
    let consumer_src =
        "<?php\nuse Acme\\VendorOnly;\nfunction use_it(VendorOnly $v) { $v->vendorOnly(); }\n";
    let consumer_path = dir.write("src/Consumer.php", consumer_src);
    let consumer_uri = path_to_uri(&consumer_path).to_string();
    let pos = lsp_pos(consumer_src, "vendorOnly()");

    let messages = run(&dir, &[hover_request(2, &consumer_uri, pos)]);
    let result = response_result(&messages, 2);
    assert!(
        result["contents"]
            .as_str()
            .unwrap_or("")
            .contains("vendorOnly"),
        "got: {result:?}"
    );
}

#[test]
fn first_party_declaration_wins_a_same_name_collision_with_vendor_context() {
    let dir = TempDir::new("first_party_wins_vendor_collision");
    dir.write("composer.json", "{}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        "<?php\nreturn ['Shared' => __DIR__ . '/../acme/Shared.php'];\n",
    );
    dir.write(
        "vendor/acme/Shared.php",
        "<?php\nclass Shared { public function vendorMethod(): int { return 1; } }\n",
    );
    dir.write(
        "src/Shared.php",
        "<?php\nclass Shared { public function ownMethod(): int { return 1; } }\n",
    );
    let use_src = "<?php function use_it(Shared $s) { $s->ownMethod(); }";
    let use_path = dir.write("src/Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "ownMethod");

    let messages = run(&dir, &[hover_request(2, &use_uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        result["contents"], "Shared::ownMethod(): int",
        "got: {result:?}"
    );
}

// A configured `phpstan-rs.toml` `paths` key is an include-only
// allowlist (mirrors `run_check_project`'s own `config.paths` handling)
// - `load_workspace_files` must scan exactly those paths instead of the
// whole workspace root when one is set, the real gap a real editor
// session surfaced: Piwigo's own `phpstan-rs.toml` deliberately omits
// `_data/` (compiled Smarty template cache) from `paths`, but the LSP
// scanned it anyway since `ResolvedWorkspaceConfig` never carried
// `config.paths` through at all before this fix.

#[test]
fn configured_paths_restrict_the_analyzed_walk_to_only_those_paths() {
    let dir = TempDir::new("configured_paths_restrict_scan");
    dir.write("phpstan-rs.toml", "paths = [\"src\"]\n");
    dir.write(
        "excluded/Ghost.php",
        "<?php class GhostClass { public function ghostMethod(): int { return 1; } }",
    );
    let use_src = "<?php function use_it(GhostClass $g) { $g->ghostMethod(); }";
    let use_path = dir.write("src/Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();

    let messages = run(
        &dir,
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": use_uri, "languageId": "php", "version": 1, "text": use_src}}),
        )],
    );
    let diags = last_publish_diagnostics_for(&messages, &use_uri);
    assert!(
        diags["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "class.notFound"),
        "GhostClass is only declared under excluded/, which paths = [\"src\"] must leave \
         unscanned - got: {diags:?}"
    );
}

#[test]
fn configured_paths_still_fully_analyze_everything_inside_them() {
    let dir = TempDir::new("configured_paths_still_scan_included");
    dir.write("phpstan-rs.toml", "paths = [\"src\"]\n");
    dir.write(
        "src/App.php",
        "<?php class App { public function ownMethod(): int { return 1; } }",
    );
    let use_src = "<?php function use_it(App $a) { $a->ownMethod(); }";
    let use_path = dir.write("src/Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "ownMethod");

    let messages = run(&dir, &[hover_request(2, &use_uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        result["contents"], "App::ownMethod(): int",
        "got: {result:?}"
    );
}

#[test]
fn opening_an_undiscovered_vendor_file_does_not_clobber_a_first_party_declaration() {
    // No composer.json at all: discover_vendor_context finds nothing,
    // so vendor/acme/Shared.php is never touched by load_workspace -
    // genuinely never-before-seen by Project until the didOpen below.
    let dir = TempDir::new("undiscovered_vendor_didopen");
    dir.write(
        "src/Shared.php",
        "<?php\nclass Shared { public function ownMethod(): int { return 1; } }\n",
    );
    let vendor_src = "<?php\nclass Shared { public function vendorMethod(): int { return 1; } }\n";
    let vendor_path = dir.write("vendor/acme/Shared.php", vendor_src);
    let vendor_uri = path_to_uri(&vendor_path).to_string();

    let use_src = "<?php function use_it(Shared $s) { $s->ownMethod(); }";
    let use_path = dir.write("src/Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "ownMethod");

    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": vendor_uri, "languageId": "php", "version": 1, "text": vendor_src}}),
            ),
            hover_request(2, &use_uri, pos),
        ],
    );
    let result = response_result(&messages, 2);
    assert_eq!(
        result["contents"], "Shared::ownMethod(): int",
        "the vendor Shared must not clobber the real one via didOpen: got: {result:?}"
    );
}
