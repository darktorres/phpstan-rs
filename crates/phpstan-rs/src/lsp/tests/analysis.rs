//! What the server publishes after an edit, a save and a deletion. An edit
//! checks the edited file and the files that depend on it. A save runs the
//! full analysis, whole-program passes included. Each sends only the sets
//! that changed since the editor was last sent them.

use super::super::*;
use super::helpers::*;
use serde_json::{Value, json};

/// Enables the dead-code check, one of the whole-program passes.
const DEAD_CODE_CONFIG: &str = "[rules]\ncheck_dead_code = true\n";

const UNDEFINED_CALL: &str = "<?php\nfunction f(): void {\n    totallyUndefinedFunction();\n}\n";

const IGNORED_CALL: &str =
    "<?php\nfunction f(): void {\n    totallyUndefinedFunction(); // @phpstan-ignore-line\n}\n";

/// Runs the server on its own thread: `before`, then `save`. Returns once
/// `uri` has been published, and then ends the session. A save's analysis
/// runs on the debounce worker after its window, and `exit` does not flush a
/// pending save, so the publish is awaited before `exit` is sent.
fn run_until_published(dir: &TempDir, before: &[Value], save: Value, uri: &str) -> Vec<Value> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let writer = Arc::new(Mutex::new(Vec::new()));
    let worker_writer = Arc::clone(&writer);
    let handle = std::thread::spawn(move || {
        run_loop(
            &rx,
            worker_writer,
            env!("CARGO_PKG_VERSION"),
            crate::app::config_schema(),
        );
    });
    let mut sent = vec![
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
    ];
    sent.extend_from_slice(before);
    sent.push(save);
    for m in &sent {
        tx.send(to_message(m.clone())).unwrap();
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while publishes_for(&decode_all(&writer.lock().unwrap()), uri).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the save never published {uri}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    tx.send(to_message(notification("exit", json!({}))))
        .unwrap();
    drop(tx);
    handle.join().unwrap();
    decode_all(&writer.lock().unwrap())
}

#[test]
fn an_edit_does_not_run_the_whole_program_checks_and_a_save_does() {
    let dir = TempDir::new("analysis_edit_then_save");
    dir.write("phpstan-rs.toml", DEAD_CODE_CONFIG);
    let defs = dir.write("defs.php", "<?php\nclass UsedClass {}\n");
    let user = dir.write("user.php", "<?php\nnew UsedClass();\n");
    let defs_uri = path_to_uri(&defs).to_string();
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        publishes_for(&initial, &defs_uri).is_empty(),
        "UsedClass is used, so nothing is unused, got: {initial:?}"
    );

    let edit = as_values(server.handle_debounced_change(
        &user,
        &path_to_uri(&user),
        "<?php\n",
        2,
        &mut || false,
    ));
    assert!(
        publishes_for(&edit, &defs_uri).is_empty(),
        "an edit leaves the whole-program checks to the next save, got: {edit:?}"
    );

    let save = as_values(server.analyse_on_save());
    let published = publishes_for(&save, &defs_uri);
    assert_eq!(
        published.len(),
        1,
        "a save publishes defs.php, got: {save:?}"
    );
    assert!(
        diagnostics_of(published[0])
            .iter()
            .any(|d| d["code"] == "class.unused"),
        "the save runs the dead-code check, got: {save:?}"
    );
}

#[test]
fn a_save_of_an_unchanged_project_publishes_nothing_new() {
    let dir = TempDir::new("analysis_unchanged_save");
    dir.write("phpstan-rs.toml", DEAD_CODE_CONFIG);
    dir.write("defs.php", "<?php\nclass UnusedClass {}\n");
    dir.write("f.php", UNDEFINED_CALL);
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        !initial.is_empty(),
        "the project has diagnostics to publish"
    );

    let save = as_values(server.analyse_on_save());
    assert!(
        save.is_empty(),
        "a save of an unchanged project changes nothing, got: {save:?}"
    );
}

#[test]
fn a_save_clears_a_whole_program_diagnostic_that_an_edit_left_in_place() {
    let dir = TempDir::new("analysis_save_clears");
    dir.write("phpstan-rs.toml", DEAD_CODE_CONFIG);
    let defs = dir.write("defs.php", "<?php\nclass UnusedClass {}\n");
    let user = dir.write("user.php", "<?php\n");
    let defs_uri = path_to_uri(&defs).to_string();
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        publishes_for(&initial, &defs_uri)
            .iter()
            .any(|m| diagnostics_of(m)
                .iter()
                .any(|d| d["code"] == "class.unused")),
        "UnusedClass is never used, got: {initial:?}"
    );

    let edit = as_values(server.handle_debounced_change(
        &user,
        &path_to_uri(&user),
        "<?php\nnew UnusedClass();\n",
        2,
        &mut || false,
    ));
    assert!(
        publishes_for(&edit, &defs_uri).is_empty(),
        "the edit does not republish defs.php, got: {edit:?}"
    );

    let save = as_values(server.analyse_on_save());
    let cleared = publishes_for(&save, &defs_uri);
    assert_eq!(
        cleared.len(),
        1,
        "the save publishes defs.php, got: {save:?}"
    );
    assert!(
        diagnostics_of(cleared[0]).is_empty(),
        "the save clears defs.php, got: {save:?}"
    );
}

#[test]
fn an_ignore_comment_suppresses_a_diagnostic_until_an_edit_removes_it() {
    let dir = TempDir::new("analysis_ignore_comment");
    let path = dir.write("f.php", IGNORED_CALL);
    let uri = path_to_uri(&path);
    let uri_text = uri.to_string();
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        publishes_for(&initial, &uri_text).is_empty(),
        "the ignore comment suppresses the only diagnostic, got: {initial:?}"
    );

    let unignored =
        as_values(server.handle_debounced_change(&path, &uri, UNDEFINED_CALL, 2, &mut || false));
    let published = publishes_for(&unignored, &uri_text);
    assert_eq!(
        published.len(),
        1,
        "removing the ignore comment publishes f.php, got: {unignored:?}"
    );
    assert!(
        diagnostics_of(published[0]).iter().any(|d| d["message"]
            .as_str()
            .unwrap()
            .contains("totallyUndefinedFunction")),
        "the undefined call is reported again, got: {unignored:?}"
    );

    let ignored =
        as_values(server.handle_debounced_change(&path, &uri, IGNORED_CALL, 3, &mut || false));
    let cleared = publishes_for(&ignored, &uri_text);
    assert_eq!(
        cleared.len(),
        1,
        "adding the ignore comment publishes f.php, got: {ignored:?}"
    );
    assert!(
        diagnostics_of(cleared[0]).is_empty(),
        "the ignore comment clears f.php, got: {ignored:?}"
    );
}

#[test]
fn a_superseded_run_sends_nothing_and_the_newer_run_sends_its_result() {
    let dir = TempDir::new("analysis_superseded_run");
    let path = dir.write("f.php", UNDEFINED_CALL);
    let uri = path_to_uri(&path);
    let uri_text = uri.to_string();
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        !publishes_for(&initial, &uri_text).is_empty(),
        "f.php has an undefined call, got: {initial:?}"
    );
    // The server keeps what it sent by the file's resolved path, which is not
    // always the spelling of the directory the file was written in.
    let resolved = composer::canonical::canonicalize(&path).unwrap();
    let sent = server
        .published
        .get(&resolved)
        .map(|held| held.diagnostics.clone())
        .expect("f.php was published");

    let superseded = server.handle_debounced_change(&path, &uri, IGNORED_CALL, 2, &mut || true);
    assert!(
        superseded.is_empty(),
        "a superseded run sends nothing, got: {superseded:?}"
    );
    assert_eq!(
        server
            .published
            .get(&resolved)
            .map(|held| &held.diagnostics),
        Some(&sent),
        "a superseded run leaves what the editor was sent unchanged"
    );

    let newer =
        as_values(server.handle_debounced_change(&path, &uri, IGNORED_CALL, 3, &mut || false));
    let cleared = publishes_for(&newer, &uri_text);
    assert_eq!(
        cleared.len(),
        1,
        "the newer run publishes f.php, got: {newer:?}"
    );
    assert!(
        diagnostics_of(cleared[0]).is_empty(),
        "the newer run's result is sent, got: {newer:?}"
    );
}

#[test]
fn deleting_a_file_clears_the_diagnostics_the_editor_was_sent() {
    let dir = TempDir::new("analysis_delete");
    let path = dir.write("f.php", UNDEFINED_CALL);
    let uri = path_to_uri(&path);
    let uri_text = uri.to_string();
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        !publishes_for(&initial, &uri_text).is_empty(),
        "f.php has an undefined call, got: {initial:?}"
    );

    std::fs::remove_file(&path).unwrap();
    let deleted = as_values(
        server.handle_watched_files_changed(vec![lsp_types::FileEvent {
            uri: uri.clone(),
            typ: lsp_types::FileChangeType::DELETED,
        }]),
    );
    let cleared = publishes_for(&deleted, &uri_text);
    assert_eq!(
        cleared.len(),
        1,
        "the deletion publishes f.php, got: {deleted:?}"
    );
    assert!(
        diagnostics_of(cleared[0]).is_empty(),
        "the deletion clears f.php, got: {deleted:?}"
    );
}

#[test]
fn a_save_on_the_debounce_worker_publishes_the_whole_program_diagnostics() {
    let dir = TempDir::new("analysis_save_on_worker");
    dir.write("phpstan-rs.toml", DEAD_CODE_CONFIG);
    let defs = dir.write("defs.php", "<?php\nclass UsedClass {}\n");
    let user = dir.write("user.php", "<?php\nnew UsedClass();\n");
    let defs_uri = path_to_uri(&defs).to_string();
    let user_uri = path_to_uri(&user).to_string();
    let before = [
        notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": user_uri, "languageId": "php", "version": 1, "text": "<?php\nnew UsedClass();\n"}}),
        ),
        notification(
            "textDocument/didChange",
            json!({"textDocument": {"uri": user_uri, "version": 2}, "contentChanges": [{"text": "<?php\n"}]}),
        ),
    ];
    let save = notification(
        "textDocument/didSave",
        json!({"textDocument": {"uri": user_uri}}),
    );

    let messages = run_until_published(&dir, &before, save, &defs_uri);
    let published = publishes_for(&messages, &defs_uri);
    assert!(
        published.iter().any(|m| diagnostics_of(m)
            .iter()
            .any(|d| d["code"] == "class.unused")),
        "the save on the worker runs the dead-code check, got: {messages:?}"
    );
}

#[test]
fn a_reload_that_turns_the_dead_code_check_on_reports_the_files_already_loaded() {
    let dir = TempDir::new("analysis_reload_dead_code");
    let defs = dir.write("defs.php", "<?php\nclass UnusedClass {}\n");
    let defs_uri = path_to_uri(&defs).to_string();
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        publishes_for(&initial, &defs_uri).is_empty(),
        "the dead-code check starts off, got: {initial:?}"
    );

    dir.write("phpstan-rs.toml", DEAD_CODE_CONFIG);
    let reload = as_values(server.reload_configuration(settings::PhpstanRsSettings {
        enable: true,
        config_path: None,
    }));
    let published = publishes_for(&reload, &defs_uri);
    assert!(
        published.iter().any(|m| diagnostics_of(m)
            .iter()
            .any(|d| d["code"] == "class.unused")),
        "the reload checks defs.php, which was loaded before it, got: {reload:?}"
    );
}
