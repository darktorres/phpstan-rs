//! Tests for check_chunk and format_signature, the deterministic checking logic in main.rs.
use super::super::check::check_chunk;
use super::helpers::*;
use phpstan_rs::registry::{self, format_signature};

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
    let sig = registry::FunctionSig {
        provenance: registry::Provenance::default(),
        params: vec![
            registry::ParamSig {
                name: "x".to_string().into(),
                ty: types::Type::Int,
                ty_native: types::Type::Int,
                variadic: false,
                has_default: false,
                default_ty: None,
                doc_ty: None,
                by_ref: false,
                has_own_doc: true,
                explicit_mixed_hint: false,
                case_refs: Vec::new(),
                closure_this_type: None,
                out_ty: None,
            },
            registry::ParamSig {
                name: "rest".to_string().into(),
                ty: types::Type::String,
                ty_native: types::Type::String,
                variadic: true,
                has_default: false,
                default_ty: None,
                doc_ty: None,
                by_ref: false,
                has_own_doc: true,
                explicit_mixed_hint: false,
                case_refs: Vec::new(),
                closure_this_type: None,
                out_ty: None,
            },
        ],
        return_type: types::Type::Bool,
        return_type_native: types::Type::Bool,
        return_has_own_doc: true,
        return_doc_ty: None,
        self_out: None,
        return_case_refs: Vec::new(),
        return_explicit_mixed_hint: false,
        no_discard: false,
        must_use_result: false,
        visibility: syntax::ast::Visibility::Public,
        is_internal: false,
        is_api: false,
        has_override_attr: false,
        attrs: Vec::new(),
        deprecated: None,
        throws: None,
        assert_if_true: Vec::new(),
        assert_if_false: Vec::new(),
        assert: Vec::new(),
        purity_dependencies: None,
        is_transitively_pure: false,
        is_stub: false,
        is_magic: false,
    };
    assert_eq!(
        format_signature("f", &sig),
        "f(int $x, string ...$rest): bool"
    );
}
