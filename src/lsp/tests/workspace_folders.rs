//! Multi-root workspaces: loading every folder at `initialize` (not just
//! the first), advertising the capability, and handling
//! `workspace/didChangeWorkspaceFolders` (batched add/remove, not a
//! per-folder loop that would reintroduce the quadratic-recompute bug
//! this item exists to fix).

use super::super::*;
use super::helpers::run_multi;
use super::helpers::*;
use serde_json::json;

#[test]
fn initialize_with_multiple_workspace_folders_loads_every_folder() {
    let a = TempDir::new("multi_root_a");
    a.write("Foo.php", "<?php class Foo {}");
    let b = TempDir::new("multi_root_b");
    let b_src = "<?php function useFoo(Foo $f) {}";
    let b_path = b.write("Use.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "Foo $f");

    let messages = run_multi(&[&a, &b], &[definition_request(2, &b_uri, pos)]);

    let def = response_result(&messages, 2);
    assert!(
        def["uri"].as_str().unwrap_or("").ends_with("Foo.php"),
        "expected definition to resolve into root a's Foo.php, got: {def:?}"
    );
}

#[test]
fn initialize_advertises_workspace_folder_capabilities() {
    let a = TempDir::new("multi_root_capabilities");
    let messages = run_multi(&[&a], &[]);
    let response = response_result(&messages, 1);
    assert_eq!(
        response["capabilities"]["workspace"]["workspaceFolders"],
        json!({"supported": true, "changeNotifications": true})
    );
}

#[test]
fn did_change_workspace_folders_added_loads_the_new_folder_immediately() {
    let a = TempDir::new("dcwf_add_a");
    let b = TempDir::new("dcwf_add_b");
    b.write("Bar.php", "<?php class Bar {}");
    let use_src = "<?php function useBar(Bar $b) {}";
    let use_path = a.write("Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "Bar $b");

    let messages = run_multi(
        &[&a],
        &[
            did_change_workspace_folders(vec![workspace_folder(&b)], vec![]),
            definition_request(2, &use_uri, pos),
        ],
    );

    let def = response_result(&messages, 2);
    assert!(
        def["uri"].as_str().unwrap_or("").ends_with("Bar.php"),
        "expected the newly-added folder's Bar.php to already be loaded: got {def:?}"
    );
}

#[test]
fn did_change_workspace_folders_with_multiple_added_folders_in_one_event_loads_both() {
    let a = TempDir::new("dcwf_multi_add_a");
    let c = TempDir::new("dcwf_multi_add_c");
    c.write("Baz.php", "<?php class Baz {}");
    let d = TempDir::new("dcwf_multi_add_d");
    let use_src = "<?php function useBaz(Baz $b) {}";
    let use_path = d.write("Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "Baz $b");

    let messages = run_multi(
        &[&a],
        &[
            did_change_workspace_folders(vec![workspace_folder(&c), workspace_folder(&d)], vec![]),
            definition_request(2, &use_uri, pos),
        ],
    );

    let def = response_result(&messages, 2);
    assert!(
        def["uri"].as_str().unwrap_or("").ends_with("Baz.php"),
        "expected both folders added in one event to be loaded: got {def:?}"
    );
}

#[test]
fn did_change_workspace_folders_removed_rechecks_surviving_dependents() {
    let a = TempDir::new("dcwf_remove_a");
    a.write(
        "Greeter.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    let b = TempDir::new("dcwf_remove_b");
    b.write(
        "Use.php",
        "<?php function useGreeter(Greeter $g) { $g->greet(); }",
    );

    let messages = run_multi(
        &[&a, &b],
        &[did_change_workspace_folders(
            vec![],
            vec![workspace_folder(&a)],
        )],
    );

    assert!(
        messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["message"].as_str().unwrap_or("").contains("Greeter")
                        || d["message"].as_str().unwrap_or("").contains("greet"))),
        "expected b's Use.php to be rechecked with Greeter now undefined, got: {messages:?}"
    );
}
