//! Tests for check_chunk and format_signature, the deterministic checking logic in main.rs.
use super::super::*;
use super::helpers::*;
use phpstan_rs::types;

#[test]
fn check_chunk_reports_a_read_error_for_a_missing_file_without_panicking() {
    let dir = TempDir::new("missing");
    let missing = dir.0.join("does_not_exist.php");
    let failures = check_chunk(std::slice::from_ref(&missing));
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, missing);
    assert!(failures[0].1.contains("read error"));
}

#[test]
fn check_chunk_reports_no_failure_for_valid_php() {
    let dir = TempDir::new("valid");
    let path = dir.write("a.php", "<?php function f(): int { return 1; }");
    assert!(check_chunk(&[path]).is_empty());
}

#[test]
fn format_signature_includes_params_variadic_marker_and_return_type() {
    let sig = FunctionSig {
        params: vec![
            registry::ParamSig {
                name: "x".to_string(),
                ty: types::Type::Int,
                variadic: false,
                has_default: false,
                by_ref: false,
            },
            registry::ParamSig {
                name: "rest".to_string(),
                ty: types::Type::String,
                variadic: true,
                has_default: false,
                by_ref: false,
            },
        ],
        return_type: types::Type::Bool,
        return_type_native: types::Type::Bool,
        no_discard: false,
        visibility: phpstan_rs::ast::Visibility::Public,
        is_internal: false,
        has_override_attr: false,
        deprecated: None,
        throws: None,
        assert_if_true: Vec::new(),
        assert_if_false: Vec::new(),
        assert: Vec::new(),
    };
    assert_eq!(
        format_signature("f", &sig),
        "f(int $x, string ...$rest): bool"
    );
}
