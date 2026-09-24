//! `textDocument/documentHighlight`: same-file occurrence highlighting
//! for classes, local variables (including `Closure`/`ArrowFn` scoping -
//! the load-bearing subtlety this item's own design centers on), and
//! member/instance method calls - driven through the real wire protocol,
//! matching navigation.rs's own test style.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

fn ranges(result: &Value) -> Vec<&Value> {
    result
        .as_array()
        .unwrap_or_else(|| panic!("expected a documentHighlight array, got: {result:?}"))
        .iter()
        .map(|h| &h["range"])
        .collect()
}

#[test]
fn initialize_advertises_document_highlight_support() {
    let dir = TempDir::new("dh_caps");
    let messages = run(&dir, &[]);
    assert_eq!(
        messages[0]["result"]["capabilities"]["documentHighlightProvider"],
        json!(true)
    );
}

#[test]
fn highlighting_a_class_resolves_every_same_file_usage_but_not_other_files() {
    let dir = TempDir::new("dh_class");
    let src = "<?php\nclass Foo {}\nfunction make(): Foo { return new Foo(); }\n";
    let path = dir.write("a.php", src);
    // A second file with its own, unrelated `Foo` - collect_all only
    // ever walks the requested file's own AST, so this contributes
    // nothing regardless of its own content.
    dir.write("b.php", "<?php\nclass Foo {}\n");
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "Foo()");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(ranges(&result).len(), 3, "got: {result:?}");
}

#[test]
fn highlighting_a_local_variable_covers_every_read_in_its_own_function() {
    let dir = TempDir::new("dh_local_var");
    let src = "<?php\nfunction f(int $x) {\n    $y = $x + 1;\n    return $x + $y;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "return ");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(&result).len(),
        3,
        "expected the param decl plus both $x reads, got: {result:?}"
    );
}

#[test]
fn highlighting_does_not_cross_into_an_uncaptured_closures_own_local_of_the_same_name() {
    let dir = TempDir::new("dh_closure_shadow");
    let src = "<?php\nfunction f() {\n    $tmp = 1;\n    $g = function () {\n        $tmp = 2;\n        return $tmp;\n    };\n    return $tmp;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$tmp = 1");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(&result).len(),
        2,
        "expected only the outer assignment and outer return, not the closure's own $tmp, got: {result:?}"
    );
}

#[test]
fn highlighting_a_captured_variable_reaches_into_the_closures_use_clause_and_body() {
    let dir = TempDir::new("dh_closure_capture");
    let src = "<?php\nfunction f($x) {\n    $g = function () use ($x) {\n        return $x;\n    };\n    return $x;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "function f(");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(&result).len(),
        4,
        "expected the param, the use($x) clause, the closure-body return, and the outer return, got: {result:?}"
    );
}

#[test]
fn highlighting_an_arrow_fns_implicit_capture_is_treated_as_the_same_variable() {
    let dir = TempDir::new("dh_arrow_fn_capture");
    let src = "<?php\nfunction f($y) {\n    $g = fn() => $y + 1;\n    return $y;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = pos_after(src, "function f(");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(&result).len(),
        3,
        "expected the param, the arrow-fn body's implicit capture, and the outer return, got: {result:?}"
    );
}

#[test]
fn highlighting_a_method_does_not_cross_into_an_unrelated_classs_same_named_method() {
    let dir = TempDir::new("dh_unrelated_class");
    let src = "<?php\nclass A { function bar(): void {} }\nclass B { function bar(): void {} }\nfunction f(A $a) { $a->bar(); }\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "bar(): void {} }\nclass B");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(&result).len(),
        2,
        "expected only A::bar's own decl and call site, not B::bar's unrelated decl, got: {result:?}"
    );
}

#[test]
fn highlighting_an_instance_method_call_finds_every_call_on_the_same_receiver_type_with_tight_spans()
 {
    let dir = TempDir::new("dh_instance_tight_span");
    let src = "<?php\nclass Foo { function bar(): void {} }\nfunction f() {\n    $x = new Foo();\n    $x->bar();\n    $x->bar();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "bar();\n    $x->bar();\n}");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    let rs = ranges(&result);
    assert_eq!(
        rs.len(),
        3,
        "expected the decl plus both call sites, got: {result:?}"
    );
    // Each call-site range must cover only the identifier "bar" (4
    // characters), never the whole `$x->bar()` call including its
    // parens - the regression check for reading `spans[0]` (the member
    // name's own sub-span) rather than `expr.span` in the MethodCall arm.
    for r in &rs {
        let start = r["start"]["character"].as_u64().unwrap();
        let end = r["end"]["character"].as_u64().unwrap();
        assert_eq!(
            end - start,
            3,
            "expected a 3-character \"bar\" span, got: {r:?}"
        );
    }
}

#[test]
fn highlighting_returns_no_response_on_an_unresolvable_position() {
    let dir = TempDir::new("dh_unresolvable");
    let src = "<?php\n// just a comment\nfunction f(): void {}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "just a comment");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2), Value::Null);
}

#[test]
fn highlighting_a_top_level_closures_local_is_scoped_correctly_outside_any_function() {
    let dir = TempDir::new("dh_top_level_closure");
    let src =
        "<?php\n$tmp = 1;\n$g = function () {\n    $tmp = 2;\n    return $tmp;\n};\necho $tmp;\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$tmp = 1");

    let messages = run(&dir, &[document_highlight_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(&result).len(),
        2,
        "expected only the top-level assignment and the trailing echo, not the closure's own $tmp, got: {result:?}"
    );
}
