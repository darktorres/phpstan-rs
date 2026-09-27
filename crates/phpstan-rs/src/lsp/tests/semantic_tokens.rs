//! `textDocument/semanticTokens/full`: push-highlighting for
//! `@deprecated`/`@internal` usage, independent of whether either
//! diagnostic gate is enabled - driven through the real wire protocol,
//! matching workspace_symbol.rs's own test style. Decodes the LSP wire
//! format's delta-encoded `u32` array by hand (not via `lsp_types`
//! itself) so a regression in `lsp::semantic_tokens::encode`'s own
//! arithmetic is caught the same way a real client would notice it.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy)]
struct DecodedToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

const DEPRECATED_BIT: u32 = 1;
const INTERNAL_BIT: u32 = 2;

fn decode(result: &Value) -> Vec<DecodedToken> {
    let data = result["data"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a data array, got: {result:?}"));
    let nums: Vec<u32> = data.iter().map(|v| v.as_u64().unwrap() as u32).collect();
    let mut out = Vec::new();
    let mut line = 0u32;
    let mut start = 0u32;
    for (i, chunk) in nums.as_chunks::<5>().0.iter().enumerate() {
        let (delta_line, delta_start, length, token_type, modifiers) =
            (chunk[0], chunk[1], chunk[2], chunk[3], chunk[4]);
        if i == 0 {
            line = delta_line;
            start = delta_start;
        } else if delta_line == 0 {
            start += delta_start;
        } else {
            line += delta_line;
            start = delta_start;
        }
        out.push(DecodedToken {
            line,
            start,
            length,
            token_type,
            modifiers,
        });
    }
    out
}

#[test]
fn a_deprecated_function_call_gets_a_token_even_with_the_diagnostic_gate_left_off() {
    let dir = TempDir::new("st_deprecated_gate_off");
    let src = "<?php\n/** @deprecated */\nfunction foo(): void {}\nfoo();\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": &uri, "languageId": "php", "version": 1, "text": src}}),
            ),
            semantic_tokens_request(3, &uri),
        ],
    );

    let result = response_result(&messages, 3);
    let tokens = decode(&result);
    assert_eq!(tokens.len(), 1, "got: {result:?}");
    assert_eq!(tokens[0].token_type, 0, "expected a Function-kind token");
    assert_eq!(tokens[0].modifiers, DEPRECATED_BIT);
    assert_eq!(tokens[0].line, 3);
    assert_eq!(
        tokens[0].start, 0,
        "expected the token to start at column 0"
    );
    assert_eq!(tokens[0].length, 3, "expected the bare \"foo\" callee span");

    let publish = last_publish_diagnostics_for(&messages, &uri);
    assert!(
        !publish["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "function.deprecated"),
        "ext.deprecation-rules.enabled is off by default - no diagnostic should fire, got: {publish:?}"
    );
}

#[test]
fn an_internal_function_called_from_outside_its_root_namespace_gets_a_token() {
    let dir = TempDir::new("st_internal_cross_ns");
    let src = "<?php\nnamespace Vendor\\Internal;\n/** @internal */\nfunction make(): void {}\n\nnamespace Other\\App;\nfunction f() {\n    \\Vendor\\Internal\\make();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    let tokens = decode(&result);
    assert_eq!(tokens.len(), 1, "got: {result:?}");
    assert_eq!(tokens[0].modifiers, INTERNAL_BIT);
}

#[test]
fn an_internal_function_called_from_within_its_own_root_namespace_gets_no_token() {
    let dir = TempDir::new("st_internal_same_ns");
    let src = "<?php\nnamespace Vendor\\Internal;\n/** @internal */\nfunction make(): void {}\n\nnamespace Vendor\\App;\nfunction f() {\n    \\Vendor\\Internal\\make();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        decode(&result).len(),
        0,
        "an in-package @internal access must never light up, got: {result:?}"
    );
}

#[test]
fn a_method_with_both_tags_gets_one_token_with_both_bits_set() {
    let dir = TempDir::new("st_both_tags");
    let src = "<?php\nnamespace Vendor\\Lib;\nclass Foo {\n    /**\n     * @deprecated\n     * @internal\n     */\n    function bar(): void {}\n}\n\nnamespace Other\\App;\nfunction f(\\Vendor\\Lib\\Foo $x) {\n    $x->bar();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    let tokens = decode(&result);
    assert_eq!(
        tokens.len(),
        1,
        "expected exactly one token, not two overlapping ones, got: {result:?}"
    );
    assert_eq!(tokens[0].token_type, 1, "expected a Method-kind token");
    assert_eq!(tokens[0].modifiers, DEPRECATED_BIT | INTERNAL_BIT);
}

#[test]
fn an_untagged_method_on_a_deprecated_class_still_gets_a_token_via_the_cascade() {
    let dir = TempDir::new("st_class_cascade_deprecated");
    let src = "<?php\nnamespace Vendor\\Lib;\n/** @deprecated */\nclass Foo {\n    function bar(): void {}\n}\n\nnamespace Other\\App;\nfunction f(\\Vendor\\Lib\\Foo $x) {\n    $x->bar();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    let tokens = decode(&result);
    assert_eq!(tokens.len(), 1, "got: {result:?}");
    assert_eq!(tokens[0].modifiers, DEPRECATED_BIT);
}

#[test]
fn an_untagged_method_on_an_internal_class_gets_no_token_from_within_its_own_root_namespace() {
    let dir = TempDir::new("st_class_cascade_internal_same_ns");
    let src = "<?php\nnamespace Vendor\\Lib;\n/** @internal */\nclass Foo {\n    function bar(): void {}\n}\n\nnamespace Vendor\\App;\nfunction f(\\Vendor\\Lib\\Foo $x) {\n    $x->bar();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        decode(&result).len(),
        0,
        "same-root-namespace access via the class-level @internal cascade must never light up, got: {result:?}"
    );
}

#[test]
fn instantiating_a_deprecated_class_gets_a_class_kind_token_at_the_bare_class_name_span() {
    let dir = TempDir::new("st_instantiation");
    let src = "<?php\n/** @deprecated */\nclass DeprecatedClass {}\nfunction f() {\n    new DeprecatedClass();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    let tokens = decode(&result);
    assert_eq!(tokens.len(), 1, "got: {result:?}");
    assert_eq!(tokens[0].token_type, 3, "expected a Class-kind token");
    assert_eq!(tokens[0].modifiers, DEPRECATED_BIT);
    assert_eq!(
        tokens[0].length,
        "DeprecatedClass".len() as u32,
        "expected class_expr.span (the bare name only), not e.span (the whole `new ...()` call)"
    );
}

#[test]
fn a_deprecated_class_constant_access_gets_a_property_kind_token() {
    let dir = TempDir::new("st_class_constant");
    let src = "<?php\nclass Foo {\n    /** @deprecated */\n    const BAR = 1;\n}\nfunction f() {\n    echo Foo::BAR;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    let tokens = decode(&result);
    assert_eq!(tokens.len(), 1, "got: {result:?}");
    assert_eq!(
        tokens[0].token_type, 2,
        "expected token_type_index(ClassConstant) == token_type_index(Property)"
    );
    assert_eq!(tokens[0].modifiers, DEPRECATED_BIT);
}

#[test]
fn the_legend_and_token_type_index_stay_in_sync() {
    let dir = TempDir::new("st_legend");
    let messages = run(&dir, &[]);
    let legend = &messages[0]["result"]["capabilities"]["semanticTokensProvider"]["legend"];
    assert_eq!(
        legend["tokenTypes"],
        json!(["function", "method", "property", "class"])
    );
    assert_eq!(legend["tokenModifiers"], json!(["deprecated", "internal"]));
}

#[test]
fn a_multi_line_call_span_degrades_to_no_token_while_the_diagnostic_still_fires() {
    let dir = TempDir::new("st_multiline");
    dir.write(
        "phpstan-rs.toml",
        "[ext.deprecation-rules]\nenabled = true\n",
    );
    let src = "<?php\nnamespace Vendor\\Lib;\nclass Foo {\n    /** @deprecated */\n    function bar(): void {}\n}\n\nnamespace Other\\App;\nfunction f(\\Vendor\\Lib\\Foo $x) {\n    $x\n        ->bar();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        decode(&result).len(),
        0,
        "a token whose span crosses a line boundary must be dropped, got: {result:?}"
    );

    let publish = last_publish_diagnostics_for(&messages, &uri);
    assert!(
        publish["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "method.deprecated"),
        "the ordinary diagnostic has no such single-line restriction and must still fire, got: {publish:?}"
    );
}

#[test]
fn delta_encoding_matches_hand_computed_values_across_two_lines() {
    let dir = TempDir::new("st_delta_arithmetic");
    let src = "<?php\n/** @deprecated */\nfunction a(): void {}\n/** @deprecated */\nfunction b(): void {}\na(); b();\na();\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    let data: Vec<u64> = result["data"]
        .as_array()
        .unwrap_or_else(|| panic!("got: {result:?}"))
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(
        data,
        vec![
            5, 0, 1, 0, 1, // "a" at line 5, col 0 - first token, absolute
            0, 5, 1, 0, 1, // "b" at line 5, col 5 - same line, delta_start=5
            1, 0, 1, 0, 1, // "a" at line 6, col 0 - new line, absolute col
        ],
        "got: {result:?}"
    );
}

#[test]
fn a_file_with_no_flagged_usage_returns_an_empty_data_array_not_null() {
    let dir = TempDir::new("st_zero_usage");
    let src = "<?php\nfunction f(): void {}\nf();\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    assert_ne!(result, Value::Null, "must be Tokens{{data: []}}, not null");
    assert_eq!(result["data"], json!([]));
}

#[test]
fn a_deprecated_method_calling_its_own_deprecated_sibling_is_suppressed_but_an_outside_caller_still_gets_a_token()
 {
    let dir = TempDir::new("st_scope_suppression");
    let src = "<?php\nclass Foo {\n    /** @deprecated */\n    function old(): void {}\n\n    /** @deprecated */\n    function alsoOld(): void {\n        $this->old();\n    }\n}\nfunction f(Foo $x) {\n    $x->old();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[semantic_tokens_request(2, &uri)]);
    let result = response_result(&messages, 2);
    let tokens = decode(&result);
    assert_eq!(
        tokens.len(),
        1,
        "expected only f()'s outside call to be flagged, not alsoOld()'s own internal self-call, got: {result:?}"
    );
    assert_eq!(tokens[0].modifiers, DEPRECATED_BIT);
    assert_eq!(
        tokens[0].line,
        lsp_pos(src, "$x->old()")["line"].as_u64().unwrap() as u32
    );
}
