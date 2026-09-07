//! Phase 17 Stage A: hover and go-to-definition, driven through the real
//! `initialize`/`textDocument/hover`/`textDocument/definition` wire
//! protocol (not `navigation::resolve_at` called directly) - the same
//! `run_loop`-over-an-in-memory-`Cursor` pattern
//! `protocol_and_session.rs` already uses, so these tests exercise the
//! exact path a real editor drives.

use super::super::*;
use super::helpers::*;
use serde_json::{Value, json};
use std::io::Cursor;

/// Loads `files` (each `(name, contents)`) into a fresh workspace via
/// `initialize`'s own `rootUri` scan, then sends `extra` requests/
/// notifications, then `exit`. Returns every response/notification
/// `run_loop` wrote.
fn run(dir: &TempDir, extra: &[Value]) -> Vec<Value> {
    let mut input = vec![request(
        1,
        "initialize",
        json!({"rootUri": dir.uri(), "capabilities": {}}),
    )];
    input.extend_from_slice(extra);
    input.push(notification("exit", json!({})));
    let bytes = encode(&input);
    let mut reader = Cursor::new(bytes);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);
    decode_all(&writer)
}

fn hover_request(id: i64, uri: &str, pos: Value) -> Value {
    request(
        id,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": pos}),
    )
}

fn definition_request(id: i64, uri: &str, pos: Value) -> Value {
    request(
        id,
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": pos}),
    )
}

fn response_result(messages: &[Value], id: i64) -> Value {
    messages
        .iter()
        .find(|m| m["id"] == id)
        .unwrap_or_else(|| panic!("no response with id {id} in {messages:?}"))["result"]
        .clone()
}

#[test]
fn initialize_advertises_hover_and_definition_support() {
    let dir = TempDir::new("caps");
    let messages = run(&dir, &[]);
    let caps = &messages[0]["result"]["capabilities"];
    assert_eq!(caps["hoverProvider"], true);
    assert_eq!(caps["definitionProvider"], true);
}

#[test]
fn hover_over_a_function_call_shows_its_signature() {
    let dir = TempDir::new("hover_fn");
    let src = "<?php function greet(string $name): string { return $name; } greet('x');";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet('x')");
    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    let contents = result["contents"].as_str().unwrap();
    assert_eq!(contents, "greet(string $name): string");
}

#[test]
fn hover_over_new_shows_class_kind_and_hierarchy() {
    let dir = TempDir::new("hover_class");
    let src = "<?php interface Greets {} class Greeter implements Greets {} $g = new Greeter();";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "Greeter()");
    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    let contents = result["contents"].as_str().unwrap();
    assert_eq!(contents, "class Greeter implements Greets");
}

#[test]
fn static_access_class_half_and_member_half_resolve_to_different_targets() {
    let dir = TempDir::new("static_access");
    let src = "<?php class Widget { const LIMIT = 5; } Widget::LIMIT;";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let class_pos = lsp_pos(src, "Widget::LIMIT");
    let member_pos = lsp_pos(src, "LIMIT;");
    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, class_pos),
            hover_request(3, &uri, member_pos),
        ],
    );
    let class_hover = response_result(&messages, 2)["contents"]
        .as_str()
        .unwrap()
        .to_string();
    let member_hover = response_result(&messages, 3)["contents"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(class_hover, "class Widget");
    assert_eq!(member_hover, "Widget::LIMIT: 5");
}

#[test]
fn a_compound_type_hint_resolves_each_class_name_independently() {
    let dir = TempDir::new("compound_hint");
    let src = "<?php class Foo {} class Bar {} function f(Foo|Bar $x) {}";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let foo_pos = lsp_pos(src, "Foo|Bar");
    let bar_pos = lsp_pos(src, "Bar $x");
    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, foo_pos),
            hover_request(3, &uri, bar_pos),
        ],
    );
    assert_eq!(response_result(&messages, 2)["contents"], "class Foo");
    assert_eq!(response_result(&messages, 3)["contents"], "class Bar");
}

#[test]
fn definition_on_extends_resolves_to_the_parent_not_the_child() {
    let dir = TempDir::new("extends_def");
    let src = "<?php class Base {} class Widget extends Base {}";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    // Land inside "Base" on the `extends` clause specifically - the
    // *second* occurrence of "Base" in the file, not its own declaration.
    let extends_pos = {
        let idx = src.rfind("Base").unwrap();
        let prefix = &src[..idx + 4];
        lsp_pos(prefix, "Base")
    };
    let messages = run(&dir, &[definition_request(2, &uri, extends_pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    // The definition location's own start must land on Base's own
    // declaration (the first "Base" in the file), not the extends
    // clause - both live on the same line here, so compare the range's
    // start column against the first occurrence's own.
    let base_decl_pos = lsp_pos(src, "Base {}");
    assert_eq!(result["range"]["start"]["line"], base_decl_pos["line"]);
    assert_eq!(
        result["range"]["start"]["character"],
        base_decl_pos["character"]
    );
}

#[test]
fn this_method_call_and_self_const_resolve_without_a_receiver_type_probe() {
    let dir = TempDir::new("this_and_self");
    let src = "<?php class Widget { const LIMIT = 1; public function helper(): void {} public function run(): void { $this->helper(); self::LIMIT; } }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let this_pos = lsp_pos(src, "helper();");
    let self_pos = lsp_pos(src, "LIMIT; }");
    let messages = run(
        &dir,
        &[
            definition_request(2, &uri, this_pos),
            definition_request(3, &uri, self_pos),
        ],
    );
    let this_result = response_result(&messages, 2);
    let self_result = response_result(&messages, 3);
    assert_ne!(
        this_result,
        Value::Null,
        "expected $this->helper() to resolve"
    );
    assert_ne!(self_result, Value::Null, "expected self::LIMIT to resolve");
}

#[test]
fn definition_resolves_across_files_even_when_the_target_was_never_opened() {
    let dir = TempDir::new("cross_file_def");
    dir.write(
        "a.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    let b_src = "<?php function useGreeter(Greeter $g) { return $g->greet(); }";
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();

    let pos = lsp_pos(b_src, "Greeter $g");
    let messages = run(&dir, &[definition_request(2, &b_uri, pos)]);
    let result = response_result(&messages, 2);
    let a_uri = path_to_uri(&dir.0.join("a.php")).to_string();
    assert_eq!(result["uri"], a_uri);
}

#[test]
fn nothing_resolvable_returns_null_not_an_error() {
    let dir = TempDir::new("nothing");
    let src = "<?php function f(int $x) { return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    // Land on the primitive type keyword `int` - filtered out of
    // `type_hint_identifiers` on purpose (it's never a real class), so
    // still a real position with nothing resolvable. (The bare parameter
    // `$x`, previously used for this test, now *does* resolve - to its
    // own declaration - since local-variable-declaration navigation
    // shipped; see the `variable_navigation` tests below.)
    let pos = lsp_pos(src, "int $x");
    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos.clone()),
            definition_request(3, &uri, pos),
        ],
    );
    assert_eq!(response_result(&messages, 2), Value::Null);
    assert_eq!(response_result(&messages, 3), Value::Null);
}

#[test]
fn definition_repopulates_after_a_did_change_moves_the_declaration() {
    let dir = TempDir::new("did_change_def");
    dir.write("a.php", "<?php class Widget {}");
    let b_src = "<?php function useWidget(Widget $w) {}";
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();
    let a_uri = path_to_uri(&dir.0.join("a.php")).to_string();

    // Move Widget's own declaration down a line via didChange - the
    // index must repopulate from the new source, not keep serving the
    // stale line-1 location load_workspace originally indexed.
    let new_a_src = "<?php\nclass Widget {}";
    let pos = lsp_pos(b_src, "Widget $w");
    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": a_uri, "version": 2},
                    "contentChanges": [{"text": new_a_src}],
                }),
            ),
            definition_request(2, &b_uri, pos),
        ],
    );
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], a_uri);
    let expected = lsp_pos(new_a_src, "Widget {}");
    assert_eq!(result["range"]["start"]["line"], expected["line"]);
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

// Phase 17 Stage B: an instance receiver's own type, resolved via a
// one-off checker probe (Checker::probe_span/probe_hit) rather than a
// name lookup - $x isn't $this, self, or any other syntactically-known
// target, so Stage A alone can't resolve it.

#[test]
fn hover_and_definition_on_an_instance_method_call_resolve_via_the_receivers_type() {
    let dir = TempDir::new("instance_method");
    let src = "<?php class Widget { public function greet(): string { return 'hi'; } } function useWidget(Widget $w): string { return $w->greet(); }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos.clone()),
            definition_request(3, &uri, pos),
        ],
    );
    assert_eq!(
        response_result(&messages, 2)["contents"],
        "Widget::greet(): string"
    );
    let def = response_result(&messages, 3);
    assert_eq!(def["uri"], uri);
    let expected = lsp_pos(src, "greet(): string { return 'hi'; }");
    assert_eq!(def["range"]["start"]["line"], expected["line"]);
    assert_eq!(def["range"]["start"]["character"], expected["character"]);
}

#[test]
fn hover_on_an_instance_property_access_resolves_via_the_receivers_type() {
    let dir = TempDir::new("instance_property");
    let src = "<?php class Widget { public int $count = 0; } function useWidget(Widget $w): int { return $w->count; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "count;");

    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    assert_eq!(
        response_result(&messages, 2)["contents"],
        "Widget::$count: int"
    );
}

#[test]
fn an_uninferrable_receiver_resolves_to_null_not_a_guess() {
    let dir = TempDir::new("uninferrable_receiver");
    // $w has no type hint at all - its type is Mixed, which type_to_class
    // deliberately never resolves to any class.
    let src = "<?php function useWidget($w) { return $w->greet(); }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos.clone()),
            definition_request(3, &uri, pos),
        ],
    );
    assert_eq!(response_result(&messages, 2), Value::Null);
    assert_eq!(response_result(&messages, 3), Value::Null);
}

// Phase 17 follow-up: closure/arrow-fn body probing. The original Stage B
// disclosure claimed this needed a different `Checker` entry point - false
// on inspection: `check_expr`'s own `Closure`/`ArrowFn` arms already
// recurse into a closure's body as part of ordinarily checking whatever
// *named* function contains the closure literal, and `lsp::navigation`'s
// `current_function` was already left pointing at that enclosing named
// function through a closure/arrow-fn (never reset) - so re-running
// `check_function` on it already reaches a receiver nested inside one.
// These are regression guards for that, not new capability being added.

#[test]
fn hover_and_definition_resolve_an_instance_call_inside_a_closure_body() {
    let dir = TempDir::new("closure_instance_probe");
    let src = "<?php class Widget { public function greet(): string { return 'hi'; } } function useWidget(Widget $w) { $f = function () use ($w) { return $w->greet(); }; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet(); };");

    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos.clone()),
            definition_request(3, &uri, pos),
        ],
    );
    assert_eq!(
        response_result(&messages, 2)["contents"],
        "Widget::greet(): string"
    );
    assert_eq!(response_result(&messages, 3)["uri"], uri);
}

#[test]
fn hover_resolves_an_instance_call_inside_an_arrow_fn_body() {
    let dir = TempDir::new("arrow_instance_probe");
    let src = "<?php class Widget { public function greet(): string { return 'hi'; } } function useWidget(Widget $w) { $f = fn() => $w->greet(); }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    assert_eq!(
        response_result(&messages, 2)["contents"],
        "Widget::greet(): string"
    );
}

// Phase 17 follow-up: a `Union`/`Intersection` receiver resolving to
// *every* class that declares the member, not just the first.

#[test]
fn hover_and_definition_on_a_union_receiver_resolve_every_declaring_class() {
    let dir = TempDir::new("union_receiver");
    let src = "<?php class Foo { public function greet(): string { return 'foo'; } } class Bar { public function greet(): string { return 'bar'; } } function f(Foo|Bar $x) { return $x->greet(); }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos.clone()),
            definition_request(3, &uri, pos),
        ],
    );
    assert_eq!(
        response_result(&messages, 2)["contents"],
        "Foo::greet(): string | Bar::greet(): string"
    );
    let def = response_result(&messages, 3);
    let locations = def.as_array().expect("expected an array of locations");
    assert_eq!(locations.len(), 2);
}

#[test]
fn definition_on_a_union_receiver_stays_a_scalar_when_only_one_side_has_the_member() {
    let dir = TempDir::new("union_receiver_partial");
    let src = "<?php class Foo { public function greet(): string { return 'foo'; } } class Bar {} function f(Foo|Bar $x) { return $x->greet(); }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let def = response_result(&messages, 2);
    assert!(
        def.is_object(),
        "expected a single Location object, got {def:?}"
    );
}

// Phase 17 follow-up: local-variable-declaration navigation - a read
// resolves to its own nearest-preceding declaration in the same scope
// (a parameter, an assignment, a `foreach` binding, or a `catch (...) $e`
// binding), a plain textual-order heuristic, not full control-flow
// analysis (see `Resolved::Variable`'s own doc comment).

#[test]
fn definition_on_a_variable_read_resolves_to_its_own_parameter() {
    let dir = TempDir::new("var_param");
    let src = "<?php function f(int $x) { return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$x) {");
    assert_eq!(result["range"]["start"]["line"], expected["line"]);
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_variable_read_resolves_to_its_nearest_preceding_assignment() {
    let dir = TempDir::new("var_reassign");
    let src = "<?php function f() { $x = 1; $x = 2; return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    // The *second* assignment, not the first.
    let expected = lsp_pos(src, "$x = 2");
    assert_eq!(result["range"]["start"]["line"], expected["line"]);
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_variable_read_resolves_to_its_foreach_binding() {
    let dir = TempDir::new("var_foreach");
    let src =
        "<?php function f(array $items) { foreach ($items as $k => $v) { echo $k; echo $v; } }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let k_pos = lsp_pos(src, "$k;");
    let v_pos = lsp_pos(src, "$v;");

    let messages = run(
        &dir,
        &[
            definition_request(2, &uri, k_pos),
            definition_request(3, &uri, v_pos),
        ],
    );
    let k_result = response_result(&messages, 2);
    let v_result = response_result(&messages, 3);
    let expected_k = lsp_pos(src, "$k =>");
    let expected_v = lsp_pos(src, "$v)");
    assert_eq!(
        k_result["range"]["start"]["character"],
        expected_k["character"]
    );
    assert_eq!(
        v_result["range"]["start"]["character"],
        expected_v["character"]
    );
}

#[test]
fn definition_on_a_variable_read_resolves_to_its_catch_binding() {
    let dir = TempDir::new("var_catch");
    let src = "<?php function f() { try { doStuff(); } catch (\\Exception $e) { echo $e; } }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$e;");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$e) {");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn a_variable_declared_in_one_function_does_not_leak_into_another() {
    let dir = TempDir::new("var_scope");
    let src = "<?php function f() { $x = 1; } function g() { return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2), Value::Null);
}

#[test]
fn the_probe_hook_does_not_perturb_ordinary_diagnostics() {
    // Regression guard that Checker::probe_span/probe_hit (Stage B) is a
    // true no-op outside an active probe: a hover request against an
    // instance-receiver call (which internally builds its own throwaway,
    // probe-armed Checker) followed by a real didOpen on the same file
    // must still publish exactly the diagnostics ordinary checking always
    // produces - proving the probe never leaks into the shared Project's
    // own checking path.
    let dir = TempDir::new("probe_no_perturb");
    let src = "<?php class Widget { public function greet(): string { return 'hi'; } } function useWidget(Widget $w): string { return $w->greet(); } function f(): int { return 'not an int'; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos),
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": src}}),
            ),
        ],
    );
    let diag_notification = messages
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("expected a publishDiagnostics notification");
    let diagnostics = diag_notification["params"]["diagnostics"]
        .as_array()
        .unwrap();
    assert!(
        diagnostics.iter().any(|d| d["code"] == "return.type"),
        "expected f()'s own real return.type mismatch to still be reported, got: {diagnostics:?}"
    );
}
