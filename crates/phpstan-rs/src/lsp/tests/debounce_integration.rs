//! VSCode plan item 2.2's own end-to-end integration tests: a rapid
//! `didChange` burst coalesces to one recheck, published via the
//! shutdown-flush's own forced drain when `exit` follows immediately -
//! contrasted with `didOpen`'s own separate, always-immediate publish -
//! and confirmation that the debounce worker's real timer path (not just
//! the shutdown flush) genuinely applies an edit on its own.

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

#[test]
fn rapid_didchange_burst_coalesces_and_the_shutdown_flush_publishes_the_final_edit() {
    let dir = TempDir::new("debounce_shutdown_flush");
    let initial_src = "<?php\nfunction f(): int { return 1; }\n";
    let first_edit = "<?php\nfunction f(): int { return 2; }\n";
    let second_edit = "<?php\nfunction f(): int { return 3; }\n";
    let path = dir.write("a.php", initial_src);
    let uri = path_to_uri(&path).to_string();

    // `run`'s own harness always appends `exit` right after `extra` -
    // exactly the "two rapid didChange immediately followed by exit"
    // shape this test needs: neither edit's own real-time debounce
    // window has any chance to elapse naturally before `exit` arrives,
    // so ONLY the shutdown-flush's own forced `drain_all` can be
    // responsible for whatever gets published for the second edit.
    let messages = run(
        &dir,
        &[
            did_open(&uri, initial_src),
            did_change(&uri, 2, first_edit),
            did_change(&uri, 3, second_edit),
        ],
    );

    let publishes = publishes_for(&messages, &uri);
    // didOpen gets its own separate, always-immediate publish (never
    // debounced) - version 1, distinct from whatever the coalesced
    // didChange burst eventually produces.
    assert_eq!(
        publishes[0]["params"]["version"],
        json!(1),
        "didOpen's own publish must be immediate and unaffected by debouncing, got: {publishes:?}"
    );
    // Exactly ONE more publish for the whole didChange burst - proving
    // the two rapid edits coalesced into a single recheck, not two.
    assert_eq!(
        publishes.len(),
        2,
        "expected didOpen's publish plus exactly one coalesced publish for the burst, got: {publishes:?}"
    );
    // That single coalesced publish reflects the SECOND (newer) edit's
    // own version, not the first - `DebounceQueue::record_edit`'s own
    // coalescing keeps the newest edit, discarding the superseded one.
    assert_eq!(
        publishes[1]["params"]["version"],
        json!(3),
        "expected the shutdown-flushed publish to reflect the newer, coalesced edit, got: {publishes:?}"
    );
}

#[test]
fn the_debounce_workers_own_timer_applies_an_edit_before_any_shutdown_flush_would() {
    let dir = TempDir::new("debounce_real_timer");
    dir.write("b.php", "<?php class Widget {}");
    let a_src = "<?php\nfunction f(): void {}\n";
    let a_path = dir.write("a.php", a_src);
    let a_uri = path_to_uri(&a_path).to_string();
    let b_uri = path_to_uri(&dir.0.join("b.php")).to_string();
    let new_a_src = "<?php\nfunction f(Widget $w): void {}\n";
    let pos = lsp_pos(new_a_src, "Widget $w");

    // `run_across_debounce` confirms (via its own sentinel request) that
    // `did_change` has already been fully QUEUED, then sleeps a real
    // margin past `DEFAULT_DEBOUNCE` before sending `after` - by that
    // point `exit` (and so the shutdown flush) has not even been sent
    // yet, so a correct answer here can only come from the debounce
    // worker's own real timer having independently fired and applied
    // the edit on its own.
    let messages = run_across_debounce(
        &dir,
        &[did_open(&a_uri, a_src), did_change(&a_uri, 2, new_a_src)],
        &[definition_request(2, &a_uri, pos)],
    );

    let result = response_result(&messages, 2);
    assert_eq!(
        result["uri"], b_uri,
        "expected the edited file's new Widget type hint to resolve, got: {result:?}"
    );
}
