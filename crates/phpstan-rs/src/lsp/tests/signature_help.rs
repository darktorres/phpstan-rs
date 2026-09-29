//! `textDocument/signatureHelp`, driven through the real wire protocol:
//! plain functions, constructors, `$this`/literal-class member calls, a
//! checker-probed instance receiver (including a `Union` receiver's
//! multiple real signatures), nested calls, and the three defects this
//! item's own design was corrected against - the claim-and-clear leak
//! (a dynamic or anonymous-class-constructor call nested inside a
//! resolvable outer call must clear to null, not leak the outer
//! signature), the pre-paren false positive (the cursor must be past the
//! callee/member name, not just anywhere in the call's own span), and
//! the `FirstClassCallable` exclusion.

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

fn labels(result: &Value) -> Vec<String> {
    result["signatures"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a signatures array, got: {result:?}"))
        .iter()
        .map(|s| s["label"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn initialize_advertises_signature_help_support() {
    let dir = TempDir::new("sh_caps");
    let messages = run(&dir, &[]);
    let triggers: Vec<&str> =
        messages[0]["result"]["capabilities"]["signatureHelpProvider"]["triggerCharacters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
    assert!(triggers.contains(&"("), "got: {triggers:?}");
    assert!(triggers.contains(&","), "got: {triggers:?}");
}

#[test]
fn signature_help_for_a_plain_function_call_shows_every_parameter() {
    let dir = TempDir::new("sh_plain_function");
    let src = "<?php\nfunction greet(string $a, string $b): void {}\ngreet('x', 'y');\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "greet('x', ");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(labels(&result), vec!["greet(string $a, string $b): void"]);
    assert_eq!(result["activeParameter"], json!(1));
}

#[test]
fn signature_help_active_parameter_after_a_trailing_comma_with_nothing_typed_yet() {
    let dir = TempDir::new("sh_trailing_comma");
    let src = "<?php\nfunction greet(string $a, string $b): void {}\ngreet('x',)\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "greet('x',");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["activeParameter"], json!(1));
}

#[test]
fn signature_help_still_on_the_first_parameter_right_before_its_own_comma() {
    let dir = TempDir::new("sh_before_comma");
    let src = "<?php\nfunction greet(string $a, string $b): void {}\ngreet('x', 'y')\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "greet('x'");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["activeParameter"], json!(0));
}

#[test]
fn signature_help_clamps_active_parameter_for_a_non_variadic_signature() {
    let dir = TempDir::new("sh_clamp");
    let src = "<?php\nfunction greet(string $a): void {}\ngreet('x', 'y', 'z')\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "greet('x', 'y', 'z'");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["activeParameter"], json!(0));
}

#[test]
fn signature_help_for_dollar_this_method_call_needs_no_probe() {
    let dir = TempDir::new("sh_this");
    let src = "<?php\nclass Foo {\n    function helper(int $x): void {}\n    function caller() {\n        $this->helper();\n    }\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "$this->helper(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(labels(&result), vec!["Foo::helper(int $x): void"]);
}

#[test]
fn signature_help_for_an_instance_receiver_uses_the_checker_probe() {
    let dir = TempDir::new("sh_instance_probe");
    let src = "<?php\nclass Greeter {\n    function greet(string $name): void {}\n}\nfunction f(Greeter $g) {\n    $g->greet();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "$g->greet(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(labels(&result), vec!["Greeter::greet(string $name): void"]);
}

#[test]
fn signature_help_offers_every_real_signature_for_a_union_receiver() {
    let dir = TempDir::new("sh_union_receiver");
    let src = "<?php\nclass A { function m(int $x): void {} }\nclass B { function m(string $y): void {} }\nfunction f(A|B $ab) {\n    $ab->m();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "$ab->m(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    let mut got = labels(&result);
    got.sort();
    assert_eq!(
        got,
        vec![
            "A::m(int $x): void".to_string(),
            "B::m(string $y): void".to_string()
        ]
    );
}

#[test]
fn signature_help_for_new_resolves_the_constructor_not_the_class() {
    let dir = TempDir::new("sh_new");
    let src = "<?php\nclass Widget {\n    function __construct(int $id) {}\n}\nnew Widget();\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "new Widget(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(labels(&result), vec!["Widget::__construct(int $id): mixed"]);
}

#[test]
fn signature_help_for_an_anonymous_class_resolves_its_own_constructor() {
    let dir = TempDir::new("sh_new_anonymous");
    let src = "<?php\n$x = new class() {\n    function __construct(int $id) {}\n};\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "new class(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    let labels = labels(&result);
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert!(labels[0].starts_with("class@anonymous/"), "{labels:?}");
    assert!(labels[0].ends_with(":2::__construct(int $id): mixed"), "{labels:?}");
}

#[test]
fn signature_help_targets_the_innermost_of_two_nested_calls() {
    let dir = TempDir::new("sh_nested");
    let src = "<?php\nfunction outer(int $a): void {}\nfunction inner(int $b): void {}\nouter(inner());\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "outer(inner(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(labels(&result), vec!["inner(int $b): void"]);
}

#[test]
fn signature_help_is_null_for_a_dynamically_called_variable() {
    let dir = TempDir::new("sh_dynamic_callee");
    let src = "<?php\nfunction f(callable $fn) {\n    $fn();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "$fn(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2), Value::Null);
}

#[test]
fn signature_help_returns_null_not_the_outer_signature_for_a_dynamic_call_nested_inside_a_resolvable_call()
 {
    let dir = TempDir::new("sh_leak_dynamic_nested");
    let src = "<?php\nfunction greet(string $s): void {}\nfunction f(callable $fn) {\n    greet($fn());\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "greet($fn(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    assert_eq!(
        response_result(&messages, 2),
        Value::Null,
        "the cursor is inside $fn(...), not greet(...) - must not leak greet's signature"
    );
}

#[test]
fn signature_help_returns_null_inside_an_anonymous_classs_own_constructor_arg_gap() {
    let dir = TempDir::new("sh_leak_anon_class");
    let src = "<?php\nfunction outer(int $a): void {}\nouter(new class(1, 2) {\n    public function m(): void {}\n});\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "new class(1, ");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    assert_eq!(
        response_result(&messages, 2),
        Value::Null,
        "the cursor is inside the anonymous class's own constructor args, not outer(...)"
    );
}

#[test]
fn signature_help_resolves_an_anonymous_classs_constructor_arg_against_the_enclosing_class() {
    let dir = TempDir::new("sh_anon_class_current_class_split");
    let src = "<?php\nclass Foo {\n    function helper(int $x): void {}\n    function make() {\n        new class($this->helper()) {};\n    }\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "new class($this->helper(");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        labels(&result),
        vec!["Foo::helper(int $x): void"],
        "the anonymous class's own constructor ARGS execute in the enclosing scope, not its own body's"
    );
}

#[test]
fn signature_help_is_null_with_the_cursor_still_on_the_callee_name_before_the_opening_paren() {
    let dir = TempDir::new("sh_pre_paren_gate");
    let call_src = "<?php\nfunction foo(int $a): void {}\nfoo(1);\n";
    let path = dir.write("a.php", call_src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(call_src, "fo");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2), Value::Null);

    let dir2 = TempDir::new("sh_pre_paren_gate_method");
    let method_src = "<?php\nclass Foo { function setup(int $a): void {} }\nfunction f(Foo $x) { $x->setup(1); }\n";
    let path2 = dir2.write("a.php", method_src);
    let uri2 = path_to_uri(&path2).to_string();
    let pos2 = pos_after(method_src, "$x->se");

    let messages2 = run(&dir2, &[signature_help_request(2, &uri2, pos2)]);
    assert_eq!(response_result(&messages2, 2), Value::Null);

    let dir3 = TempDir::new("sh_pre_paren_gate_new");
    let new_src = "<?php\nclass Widget { function __construct(int $id) {} }\nnew Widget(1);\n";
    let path3 = dir3.write("a.php", new_src);
    let uri3 = path_to_uri(&path3).to_string();
    let pos3 = pos_after(new_src, "new Wid");

    let messages3 = run(&dir3, &[signature_help_request(2, &uri3, pos3)]);
    assert_eq!(response_result(&messages3, 2), Value::Null);
}

#[test]
fn signature_help_is_null_inside_a_first_class_callable_reference() {
    let dir = TempDir::new("sh_fcc");
    let src = "<?php\nfunction foo(int $a): void {}\n$f = foo(...);\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "foo(.");

    let messages = run(&dir, &[signature_help_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2), Value::Null);
}

#[test]
fn signature_help_survives_a_currently_unparseable_document_via_the_last_good_parse() {
    let dir = TempDir::new("sh_last_good_parse");
    let valid_src =
        "<?php\nfunction greet(string $a): void {}\ngreet('x');\nfunction other(): void {}\n";
    // Break parsing somewhere else entirely - the call site's own
    // coordinates are unaffected.
    let edited_src = "<?php\nfunction greet(string $a): void {}\ngreet('x');\nfunction other() {\n";
    let path = dir.write("a.php", valid_src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[
            did_open(&uri, valid_src),
            did_change(&uri, 2, edited_src),
            signature_help_request(3, &uri, pos_after(edited_src, "greet('x'")),
        ],
    );

    let result = response_result(&messages, 3);
    assert_eq!(labels(&result), vec!["greet(string $a): void"]);
}
