//! Multi-root workspaces: loading every folder at `initialize` (not just
//! the first), advertising the capability, and handling
//! `workspace/didChangeWorkspaceFolders` (batched add/remove, not a
//! per-folder loop that would reintroduce the quadratic-recompute bug
//! this item exists to fix).

use super::super::*;
use super::configuration::DEPRECATED_FIXTURE;
use super::helpers::run_multi;
use super::helpers::*;
use serde_json::{Value, json};

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

fn reports_deprecation(messages: &[Value], uri: &str) -> bool {
    messages.iter().any(|m| {
        m["method"] == "textDocument/publishDiagnostics"
            && m["params"]["uri"] == uri
            && m["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "function.deprecated")
    })
}

#[test]
fn nested_folders_publish_a_file_from_the_folder_that_owns_it() {
    let outer = TempDir::new("nested_outer");
    outer.write(
        "phpstan-rs.toml",
        "[ext.deprecation-rules]\nenabled = true\n",
    );
    let control = outer.write(
        "Control.php",
        "<?php\n/** @deprecated use newControl() instead */\nfunction controlOld(): void {}\ncontrolOld();\n",
    );
    let inner = outer.write("inner/Use.php", DEPRECATED_FIXTURE);
    let control_uri = path_to_uri(&control).to_string();
    let inner_uri = path_to_uri(&inner).to_string();

    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    server.add_workspace_folders(&[outer.0.clone(), outer.0.join("inner")]);
    let messages: Vec<Value> = server
        .publish_workspace_diagnostics()
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .collect();

    // The outer folder enables the deprecation rule and lists `inner/Use.php`
    // too, but the inner folder owns the file and does not enable the rule.
    assert!(
        reports_deprecation(&messages, &control_uri),
        "the outer folder's own file must report the deprecated call, got: {messages:?}"
    );
    assert!(
        !reports_deprecation(&messages, &inner_uri),
        "the owning folder's configuration must decide the file's diagnostics, got: {messages:?}"
    );
}

#[test]
fn removing_a_folder_keeps_checking_its_open_document() {
    let a = TempDir::new("open_remove_a");
    let b = TempDir::new("open_remove_b");
    let text = "<?php function useMissing() { missing_function(); }";
    let b_use = b.write("Use.php", text);
    let uri = path_to_uri(&b_use).to_string();

    let messages = run_multi(
        &[&a, &b],
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": text}}),
            ),
            did_change_workspace_folders(vec![], vec![workspace_folder(&b)]),
        ],
    );

    // The editor still has the document open, so it stays checked outside
    // every folder.
    let last = last_publish_diagnostics_for(&messages, &uri);
    assert!(
        last["params"]["diagnostics"]
            .to_string()
            .contains("missing_function"),
        "an open document outside every folder must keep its diagnostics, got: {last:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_document_opened_through_a_link_into_another_folder_is_checked_by_that_folder() {
    // `a/link` names `b/real`, so the document belongs to `b`, and `b`'s
    // configuration decides its diagnostics.
    let a = TempDir::new("link_owner_a");
    let b = TempDir::new("link_owner_b");
    b.write(
        "phpstan-rs.toml",
        "[ext.deprecation-rules]\nenabled = true\n",
    );
    b.write("real/Use.php", DEPRECATED_FIXTURE);
    std::os::unix::fs::symlink(b.0.join("real"), a.0.join("link")).unwrap();
    let link_uri = path_to_uri(&a.0.join("link/Use.php")).to_string();

    let messages = run_multi(
        &[&a, &b],
        &[notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": link_uri, "languageId": "php", "version": 1, "text": DEPRECATED_FIXTURE}}),
        )],
    );

    assert!(
        reports_deprecation(&messages, &link_uri),
        "the folder that owns the link's target must check the document, got: {messages:?}"
    );
}

#[test]
fn a_reload_that_changes_one_folders_file_set_leaves_the_other_folder_alone() {
    let a = TempDir::new("reload_set_a");
    let a_base = composer::canonical::canonicalize(&a.0).unwrap();
    a.write("src/Kept.php", "<?php class Kept {}");
    a.write("lib/Dropped.php", "<?php class Dropped {}");
    let b = TempDir::new("reload_set_b");
    let b_base = composer::canonical::canonicalize(&b.0).unwrap();
    b.write("Other.php", "<?php class Other {}");

    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    server.add_workspace_folders(&[a.0.clone(), b.0.clone()]);
    assert!(
        server.folders[0]
            .slot
            .project
            .is_loaded(&a_base.join("lib/Dropped.php"))
    );

    a.write("phpstan-rs.toml", "paths = [\"src\"]\n");
    server.reload_configuration(settings::PhpstanRsSettings {
        enable: true,
        config_path: None,
    });

    assert!(
        !server.folders[0]
            .slot
            .project
            .is_loaded(&a_base.join("lib/Dropped.php")),
        "a file the reloaded configuration no longer lists must leave its folder's project"
    );
    assert!(
        server.folders[0]
            .slot
            .project
            .is_loaded(&a_base.join("src/Kept.php"))
    );
    assert!(
        server.folders[1]
            .slot
            .project
            .is_loaded(&b_base.join("Other.php")),
        "the other folder's files must stay loaded"
    );
}
