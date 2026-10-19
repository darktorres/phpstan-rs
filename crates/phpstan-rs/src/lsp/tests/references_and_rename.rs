//! `textDocument/references`/`textDocument/rename`, driven through the
//! real wire protocol - built on `Project::use_sites`'s two-tier index
//! (`project::tests::use_site_index` exercises that index directly;
//! this file exercises the LSP-layer resolution/aggregation/edit-
//! building on top of it).

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

fn ranges(result: &Value) -> Vec<&Value> {
    result
        .as_array()
        .unwrap_or_else(|| panic!("expected a Location array, got: {result:?}"))
        .iter()
        .map(|l| &l["range"])
        .collect()
}

#[test]
fn initialize_advertises_references_and_rename_support() {
    let dir = TempDir::new("refs_caps");
    let messages = run(&dir, &[]);
    assert_eq!(
        messages[0]["result"]["capabilities"]["referencesProvider"],
        json!(true)
    );
    assert_eq!(
        messages[0]["result"]["capabilities"]["renameProvider"],
        json!(true)
    );
}

#[test]
fn references_finds_every_call_site_of_a_function_across_files() {
    let dir = TempDir::new("refs_function_cross_file");
    let a_src = "<?php\nfunction greet(): void {}\n";
    let b_src = "<?php\ngreet();\ngreet();\n";
    dir.write("a.php", a_src);
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "greet();\ngreet()");

    let messages = run(&dir, &[references_request(2, &b_uri, pos, false)]);
    let result = response_result(&messages, 2);
    assert_eq!(ranges(result).len(), 2, "got: {result:?}");
}

#[test]
fn references_with_include_declaration_also_returns_the_declaration_site() {
    let dir = TempDir::new("refs_include_decl");
    let a_src = "<?php\nfunction greet(): void {}\n";
    let b_src = "<?php\ngreet();\ngreet();\n";
    dir.write("a.php", a_src);
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "greet();\ngreet()");

    let messages = run(&dir, &[references_request(2, &b_uri, pos, true)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(result).len(),
        3,
        "expected both call sites plus the declaration, got: {result:?}"
    );
}

#[test]
fn references_on_an_inherited_method_excludes_an_unrelated_same_named_method() {
    let dir = TempDir::new("refs_unrelated_method");
    let src = "<?php\nclass A { function bar(): void {} }\nclass B { function bar(): void {} }\nfunction f(A $a) { $a->bar(); }\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "bar(): void {} }\nclass B");

    let messages = run(&dir, &[references_request(2, &uri, pos, false)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(result).len(),
        1,
        "expected only A::bar's own call site, not B::bar's unrelated declaration, got: {result:?}"
    );
}

#[test]
fn references_on_a_union_receiver_member_call_lists_every_real_owner() {
    let dir = TempDir::new("refs_union_receiver");
    let src = "<?php\nclass Foo { function same(): void {} }\nclass Bar { function same(): void {} }\nfunction both(Foo|Bar $x) { $x->same(); }\nfunction onlyFoo(Foo $y) { $y->same(); }\nfunction onlyBar(Bar $z) { $z->same(); }\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "same(); }\nfunction onlyFoo");

    let messages = run(&dir, &[references_request(2, &uri, pos, false)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        ranges(result).len(),
        3,
        "expected the union call itself plus both single-typed calls (Foo's and Bar's), got: {result:?}"
    );
}

#[test]
fn rename_and_references_return_null_for_a_local_variable_cursor() {
    let dir = TempDir::new("refs_local_var");
    let src = "<?php\nfunction f() {\n    $x = 1;\n    return $x;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x;");

    let messages = run(
        &dir,
        &[
            references_request(2, &uri, pos.clone(), false),
            rename_request(3, &uri, pos, "y"),
        ],
    );
    assert_eq!(*response_result(&messages, 2), Value::Null);
    assert_eq!(*response_result(&messages, 3), Value::Null);
}

#[test]
fn rename_a_function_rewrites_its_declaration_and_every_call_site_across_files() {
    let dir = TempDir::new("rename_function_cross_file");
    let a_src = "<?php\nfunction greet(): void {}\n";
    let b_src = "<?php\ngreet();\ngreet();\n";
    let a_path = dir.write("a.php", a_src);
    let b_path = dir.write("b.php", b_src);
    let a_uri = path_to_uri(&a_path).to_string();
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "greet();\ngreet()");

    let messages = run(&dir, &[rename_request(2, &b_uri, pos, "hello")]);
    let result = response_result(&messages, 2);
    let changes = &result["changes"];
    assert_eq!(
        changes[&a_uri].as_array().unwrap().len(),
        1,
        "expected the declaration site alone in a.php, got: {result:?}"
    );
    assert_eq!(
        changes[&b_uri].as_array().unwrap().len(),
        2,
        "expected both call sites in b.php, got: {result:?}"
    );
    for edit in changes[&b_uri].as_array().unwrap() {
        assert_eq!(edit["newText"], json!("hello"));
    }
}

#[test]
fn rename_a_class_updates_type_hints_and_instantiations_but_not_an_unrelated_class() {
    let dir = TempDir::new("rename_class");
    let a_src = "<?php\nclass Foo {}\nclass Bar {}\n";
    let b_src = "<?php\nfunction f(Foo $x) {\n    new Foo();\n    new Bar();\n}\n";
    let a_path = dir.write("a.php", a_src);
    let b_path = dir.write("b.php", b_src);
    let a_uri = path_to_uri(&a_path).to_string();
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "Foo $x");

    let messages = run(&dir, &[rename_request(2, &b_uri, pos, "Renamed")]);
    let result = response_result(&messages, 2);
    let changes = &result["changes"];
    assert_eq!(
        changes[&a_uri].as_array().unwrap().len(),
        1,
        "expected only Foo's own declaration in a.php, got: {result:?}"
    );
    assert_eq!(
        changes[&b_uri].as_array().unwrap().len(),
        2,
        "expected the type hint and the `new Foo()` site, not `new Bar()`, got: {result:?}"
    );
}

#[test]
fn rename_refuses_a_class_used_through_an_import() {
    let dir = TempDir::new("rename_refuse_import");
    let a_src = "<?php\nnamespace App;\nclass Foo {}\n";
    let b_src = "<?php\nnamespace Other;\nuse App\\Foo;\nfunction f(Foo $x) {}\n";
    dir.write("a.php", a_src);
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "Foo $x");

    let messages = run(&dir, &[rename_request(2, &b_uri, pos, "Renamed")]);
    assert!(
        response_result(&messages, 2).is_null(),
        "a rename that leaves the import resolving to nothing must be refused, got: {messages:?}"
    );
}

#[test]
fn rename_refuses_a_class_used_through_an_alias() {
    let dir = TempDir::new("rename_refuse_alias");
    let a_src = "<?php\nnamespace App;\nclass Foo {}\n";
    let b_src = "<?php\nnamespace Other;\nuse App\\Foo as Bar;\nfunction f(Bar $x) {}\n";
    dir.write("a.php", a_src);
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "Bar $x");

    let messages = run(&dir, &[rename_request(2, &b_uri, pos, "Renamed")]);
    assert!(
        response_result(&messages, 2).is_null(),
        "a rename of a class used through an alias must be refused, got: {messages:?}"
    );
}

#[test]
fn rename_refuses_a_class_referenced_by_a_qualified_name() {
    let dir = TempDir::new("rename_refuse_qualified");
    let a_src = "<?php\nnamespace App;\nclass Foo {}\n";
    let b_src = "<?php\nfunction f(\\App\\Foo $x) {}\n";
    dir.write("a.php", a_src);
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();
    let pos = lsp_pos(b_src, "App\\Foo $x");

    let messages = run(&dir, &[rename_request(2, &b_uri, pos, "Renamed")]);
    assert!(
        response_result(&messages, 2).is_null(),
        "a rename of a class referenced by a qualified name must be refused, got: {messages:?}"
    );
}
