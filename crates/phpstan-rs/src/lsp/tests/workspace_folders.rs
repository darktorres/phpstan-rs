//! Multi-root workspaces: each folder has its own session and project,
//! with its own configuration, and a document belongs to the deepest folder
//! that contains it. Adding a folder is a new session load, removing one
//! drops its project; the capability is advertised and
//! `workspace/didChangeWorkspaceFolders` handled.

use super::super::*;
use super::helpers::run_multi;
use super::helpers::*;
use serde_json::{Value, json};

fn publishes_for<'a>(messages: &'a [Value], uri: &str) -> Vec<&'a Value> {
    messages
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .collect()
}

fn codes_of(publish: &Value) -> Vec<&str> {
    publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect()
}

#[test]
fn initialize_with_multiple_workspace_folders_loads_every_folder() {
    let a = TempDir::new("multi_root_a");
    let a_src = "<?php class Foo {}\nfunction useFoo(Foo $f) {}";
    let a_path = a.write("Foo.php", a_src);
    let b = TempDir::new("multi_root_b");
    let b_src = "<?php class Bar {}\nfunction useBar(Bar $b) {}";
    let b_path = b.write("Bar.php", b_src);

    let messages = run_multi(
        &[&a, &b],
        &[
            definition_request(
                2,
                &path_to_uri(&a_path).to_string(),
                lsp_pos(a_src, "Foo $f"),
            ),
            definition_request(
                3,
                &path_to_uri(&b_path).to_string(),
                lsp_pos(b_src, "Bar $b"),
            ),
        ],
    );

    let in_a = response_result(&messages, 2);
    assert!(
        in_a["uri"].as_str().unwrap_or("").ends_with("Foo.php"),
        "expected folder a's definition to resolve in a, got: {in_a:?}"
    );
    let in_b = response_result(&messages, 3);
    assert!(
        in_b["uri"].as_str().unwrap_or("").ends_with("Bar.php"),
        "expected folder b's definition to resolve in b, got: {in_b:?}"
    );
}

#[test]
fn a_folder_does_not_see_the_classes_of_another() {
    let a = TempDir::new("multi_root_isolated_a");
    a.write("Foo.php", "<?php class Foo {}");
    let b = TempDir::new("multi_root_isolated_b");
    let b_path = b.write("Use.php", "<?php function useFoo(Foo $f) {}");

    let messages = run_multi(&[&a, &b], &[]);

    let published = publishes_for(&messages, &path_to_uri(&b_path).to_string());
    assert!(
        published
            .iter()
            .any(|m| codes_of(m).contains(&"class.notFound")),
        "expected b's Use.php not to resolve Foo, a class of folder a, got: {messages:?}"
    );
}

#[test]
fn each_folder_has_its_own_configuration() {
    let a = TempDir::new("multi_root_config_a");
    let fixture = "<?php\n/** @deprecated use newFunc() instead */\nfunction oldFunc(): void {}\noldFunc();\n";
    let a_path = a.write("a.php", fixture);
    let b = TempDir::new("multi_root_config_b");
    b.write(
        "phpstan-rs.toml",
        "[ext.deprecation-rules]\nenabled = true\n",
    );
    let b_path = b.write("b.php", fixture);

    let messages = run_multi(&[&a, &b], &[]);

    let in_b = publishes_for(&messages, &path_to_uri(&b_path).to_string());
    assert!(
        in_b.iter()
            .any(|m| codes_of(m).contains(&"function.deprecated")),
        "expected b's own configuration to apply to b, got: {messages:?}"
    );
    let in_a = publishes_for(&messages, &path_to_uri(&a_path).to_string());
    assert!(
        !in_a
            .iter()
            .any(|m| codes_of(m).contains(&"function.deprecated")),
        "expected b's configuration not to apply to a, got: {messages:?}"
    );
}

#[test]
fn a_document_belongs_to_the_deepest_folder_that_contains_it() {
    let outer = TempDir::new("multi_root_nested");
    let outer_path = outer.write("Outer.php", "<?php class Outer {}");
    let inner_path = outer.write("inner/Inner.php", "<?php function f(Missing $m) {}");
    let outside = TempDir::new("multi_root_outside");
    let outside_path = outside.write("Outside.php", "<?php class Outside {}");

    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let messages: Vec<Value> = server
        .add_workspace_folders(&[outer.0.clone(), outer.0.join("inner")])
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .collect();

    let root_of = |path: &std::path::Path| {
        server
            .folder_of(path)
            .and_then(|index| server.folders[index].root.clone())
    };
    assert_eq!(root_of(&outer_path), Some(outer.0.clone()));
    assert_eq!(root_of(&inner_path), Some(outer.0.join("inner")));
    assert_eq!(root_of(&outside_path), None);
    assert_eq!(
        publishes_for(&messages, &path_to_uri(&inner_path).to_string()).len(),
        1,
        "the outer folder lists the inner folder's files but they are not its documents, got: {messages:?}"
    );
}

#[test]
fn a_document_outside_every_folder_is_served() {
    let a = TempDir::new("multi_root_outside_a");
    a.write("A.php", "<?php class A {}");
    let outside = TempDir::new("multi_root_outside_doc");
    let src = "<?php class Lone {}\nfunction f(Lone $l) {}";
    let path = outside.write("Lone.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run_multi(
        &[&a],
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": src}}),
            ),
            definition_request(2, &uri, lsp_pos(src, "Lone $l")),
        ],
    );

    let def = response_result(&messages, 2);
    assert!(
        def["uri"].as_str().unwrap_or("").ends_with("Lone.php"),
        "expected the document outside the folders to be served, got: {def:?}"
    );
}

#[test]
fn adding_a_folder_moves_the_documents_open_in_it_there() {
    let a = TempDir::new("multi_root_move_a");
    a.write("A.php", "<?php class A {}");
    let b = TempDir::new("multi_root_move_b");
    let path = b.write("B.php", "<?php class OnDisk {}");
    let uri = path_to_uri(&path);

    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    server.add_workspace_folders(std::slice::from_ref(&a.0));
    server.handle_open_or_change(&uri, "<?php class InEditor {}", Some(1));
    assert!(server.unrooted.open_documents.contains(&path));

    server.add_workspace_folders(std::slice::from_ref(&b.0));

    assert!(!server.unrooted.open_documents.contains(&path));
    let folder = &server.folders[server.folder_of(&path).unwrap()];
    assert!(folder.open_documents.contains(&path));
    assert!(folder.project.registry().classes.contains_key("InEditor"));
    assert!(!folder.project.registry().classes.contains_key("OnDisk"));
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
    let use_src = "<?php class Bar {}\nfunction useBar(Bar $b) {}";
    let use_path = b.write("Bar.php", use_src);
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
        "expected the newly-added folder to already be loaded: got {def:?}"
    );
}

#[test]
fn did_change_workspace_folders_with_multiple_added_folders_in_one_event_loads_both() {
    let a = TempDir::new("dcwf_multi_add_a");
    let c = TempDir::new("dcwf_multi_add_c");
    let c_src = "<?php class Baz {}\nfunction useBaz(Baz $b) {}";
    let c_path = c.write("Baz.php", c_src);
    let d = TempDir::new("dcwf_multi_add_d");
    let d_src = "<?php class Qux {}\nfunction useQux(Qux $q) {}";
    let d_path = d.write("Qux.php", d_src);

    let messages = run_multi(
        &[&a],
        &[
            did_change_workspace_folders(vec![workspace_folder(&c), workspace_folder(&d)], vec![]),
            definition_request(
                2,
                &path_to_uri(&c_path).to_string(),
                lsp_pos(c_src, "Baz $b"),
            ),
            definition_request(
                3,
                &path_to_uri(&d_path).to_string(),
                lsp_pos(d_src, "Qux $q"),
            ),
        ],
    );

    let in_c = response_result(&messages, 2);
    assert!(
        in_c["uri"].as_str().unwrap_or("").ends_with("Baz.php"),
        "expected both folders added in one event to be loaded: got {in_c:?}"
    );
    let in_d = response_result(&messages, 3);
    assert!(
        in_d["uri"].as_str().unwrap_or("").ends_with("Qux.php"),
        "expected both folders added in one event to be loaded: got {in_d:?}"
    );
}

#[test]
fn did_change_workspace_folders_removed_drops_the_folder_and_clears_what_it_published() {
    let a = TempDir::new("dcwf_remove_a");
    let a_path = a.write("Bad.php", "<?php function f(Missing $m) {}");
    let a_uri = path_to_uri(&a_path).to_string();
    let b = TempDir::new("dcwf_remove_b");
    let b_path = b.write("Use.php", "<?php function g(Absent $m) {}");
    let b_uri = path_to_uri(&b_path).to_string();

    let messages = run_multi(
        &[&a, &b],
        &[did_change_workspace_folders(
            vec![],
            vec![workspace_folder(&a)],
        )],
    );

    let in_a = publishes_for(&messages, &a_uri);
    assert!(
        in_a.len() == 2 && !codes_of(in_a[0]).is_empty() && codes_of(in_a[1]).is_empty(),
        "expected a's findings published, then cleared with the folder, got: {in_a:?}"
    );
    assert_eq!(
        publishes_for(&messages, &b_uri).len(),
        1,
        "expected b untouched by a's removal, got: {messages:?}"
    );
}
