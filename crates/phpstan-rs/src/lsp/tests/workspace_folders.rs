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
    let a_foo = a.write("Foo.php", "<?php class Foo {}");
    let a_src = "<?php function useFoo(Foo $f) {}";
    let a_use = a.write("Use.php", a_src);
    let b = TempDir::new("multi_root_b");
    let b_foo = b.write("Foo.php", "<?php class Foo {}");
    let b_src = "<?php function useFoo(Foo $f) {}";
    let b_use = b.write("Use.php", b_src);

    let messages = run_multi(
        &[&a, &b],
        &[
            definition_request(
                2,
                &path_to_uri(&a_use).to_string(),
                lsp_pos(a_src, "Foo $f"),
            ),
            definition_request(
                3,
                &path_to_uri(&b_use).to_string(),
                lsp_pos(b_src, "Foo $f"),
            ),
        ],
    );

    // Each folder is loaded into its own project, so `Foo` resolves within
    // the folder whose file uses it.
    assert_eq!(
        response_result(&messages, 2)["uri"],
        json!(path_to_uri(&a_foo).to_string())
    );
    assert_eq!(
        response_result(&messages, 3)["uri"],
        json!(path_to_uri(&b_foo).to_string())
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
    let bar = b.write("Bar.php", "<?php class Bar {}");
    let use_src = "<?php function useBar(Bar $b) {}";
    let use_path = b.write("Use.php", use_src);
    let use_uri = path_to_uri(&use_path).to_string();
    let pos = lsp_pos(use_src, "Bar $b");

    let messages = run_multi(
        &[&a],
        &[
            did_change_workspace_folders(vec![workspace_folder(&b)], vec![]),
            definition_request(2, &use_uri, pos),
        ],
    );

    assert_eq!(
        response_result(&messages, 2)["uri"],
        json!(path_to_uri(&bar).to_string()),
        "expected the newly-added folder's own Bar.php to be loaded already"
    );
}

#[test]
fn did_change_workspace_folders_with_multiple_added_folders_in_one_event_loads_both() {
    let a = TempDir::new("dcwf_multi_add_a");
    let c = TempDir::new("dcwf_multi_add_c");
    let baz = c.write("Baz.php", "<?php class Baz {}");
    let c_src = "<?php function useBaz(Baz $b) {}";
    let c_use = c.write("Use.php", c_src);
    let d = TempDir::new("dcwf_multi_add_d");
    let qux = d.write("Qux.php", "<?php class Qux {}");
    let d_src = "<?php function useQux(Qux $q) {}";
    let d_use = d.write("Use.php", d_src);

    let messages = run_multi(
        &[&a],
        &[
            did_change_workspace_folders(vec![workspace_folder(&c), workspace_folder(&d)], vec![]),
            definition_request(
                2,
                &path_to_uri(&c_use).to_string(),
                lsp_pos(c_src, "Baz $b"),
            ),
            definition_request(
                3,
                &path_to_uri(&d_use).to_string(),
                lsp_pos(d_src, "Qux $q"),
            ),
        ],
    );

    assert_eq!(
        response_result(&messages, 2)["uri"],
        json!(path_to_uri(&baz).to_string()),
        "expected the first folder added in one event to be loaded"
    );
    assert_eq!(
        response_result(&messages, 3)["uri"],
        json!(path_to_uri(&qux).to_string()),
        "expected the second folder added in one event to be loaded"
    );
}

#[test]
fn did_change_workspace_folders_removed_clears_its_files_and_keeps_the_others() {
    let a = TempDir::new("dcwf_remove_a");
    let a_use = a.write(
        "Use.php",
        "<?php function useMissing() { missing_function(); }",
    );
    let b = TempDir::new("dcwf_remove_b");
    b.write(
        "Greeter.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    let b_use = b.write(
        "Use.php",
        "<?php function useGreeter(Greeter $g) { $g->nope(); }",
    );

    let messages = run_multi(
        &[&a, &b],
        &[did_change_workspace_folders(
            vec![],
            vec![workspace_folder(&a)],
        )],
    );

    // The removed folder's project goes with it, so its file's last publish
    // is empty.
    let a_uri = path_to_uri(&a_use).to_string();
    assert_eq!(
        last_publish_diagnostics_for(&messages, &a_uri)["params"]["diagnostics"],
        json!([]),
        "a removed folder's file must be cleared, got: {messages:?}"
    );
    // The surviving folder keeps its own diagnostic.
    let b_uri = path_to_uri(&b_use).to_string();
    let b_diagnostics =
        last_publish_diagnostics_for(&messages, &b_uri)["params"]["diagnostics"].to_string();
    assert!(
        b_diagnostics.contains("nope"),
        "the surviving folder must keep its own diagnostics, got: {b_diagnostics}"
    );
}
