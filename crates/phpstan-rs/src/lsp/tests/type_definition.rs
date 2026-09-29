//! `textDocument/typeDefinition` - jumps from an
//! inferred `Type` to that type's own class declaration, not to
//! wherever a clicked member/name itself is declared (that's ordinary
//! `textDocument/definition`, covered by `navigation.rs`'s own tests).

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::Value;

#[test]
fn initialize_advertises_type_definition_support() {
    let dir = TempDir::new("caps");
    let messages = run(&dir, &[]);
    let caps = &messages[0]["result"]["capabilities"];
    assert_eq!(caps["typeDefinitionProvider"], true);
}

#[test]
fn a_bare_local_variable_resolves_to_its_inferred_class() {
    let dir = TempDir::new("bare_var");
    let src = "<?php class Foo {} function f() { $x = new Foo(); $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");
    let messages = run(&dir, &[type_definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let foo_decl_pos = lsp_pos(src, "Foo {}");
    assert_eq!(result["range"]["start"]["line"], foo_decl_pos["line"]);
    assert_eq!(
        result["range"]["start"]["character"],
        foo_decl_pos["character"]
    );
}

#[test]
fn a_member_access_receiver_resolves_to_the_receivers_own_class_not_the_declaring_ancestor() {
    let dir = TempDir::new("receiver_class");
    // Regular `textDocument/definition` on `bar()` jumps to `Base::bar`
    // (where it's declared); `typeDefinition` must instead jump to
    // `Foo` (the receiver `$x`'s own inferred type) - a real, different
    // answer whenever the member is inherited, not just a rename of the
    // same result.
    let src = "<?php class Base { public function bar(): void {} } class Foo extends Base {} function f(Foo $x) { $x->bar(); }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "bar(); }");
    let messages = run(&dir, &[type_definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let foo_decl_pos = lsp_pos(src, "Foo extends Base");
    assert_eq!(result["range"]["start"]["line"], foo_decl_pos["line"]);
    assert_eq!(
        result["range"]["start"]["character"],
        foo_decl_pos["character"]
    );
}

#[test]
fn a_union_typed_receiver_resolves_to_every_real_class() {
    let dir = TempDir::new("union_receiver");
    let src = "<?php class Foo { public function bar(): void {} } class Bar { public function bar(): void {} } function f(Foo|Bar $x) { $x->bar(); }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "bar(); }");
    let messages = run(&dir, &[type_definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    let locs = result.as_array().expect("expected an array response");
    assert_eq!(locs.len(), 2, "got: {result:?}");
}

#[test]
fn a_plain_class_name_reference_declines() {
    let dir = TempDir::new("class_name_decline");
    let src = "<?php class Foo {} function f(Foo $x) {}";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "Foo $x");
    let messages = run(&dir, &[type_definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result, Value::Null);
}

#[test]
fn a_scalar_typed_variable_has_no_class_to_jump_to() {
    let dir = TempDir::new("scalar_var");
    let src = "<?php function f() { $x = 1; $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");
    let messages = run(&dir, &[type_definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result, Value::Null);
}
