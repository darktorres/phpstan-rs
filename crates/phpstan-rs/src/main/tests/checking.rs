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
    let tokens = syntax::lexer::Lexer::new("<?php function f(int $x, string ...$rest): bool {}")
        .tokenize_with_spans();
    let program = syntax::parser::Parser::new(tokens).parse_program().unwrap();
    let registry = registry::Registry::build(&program);
    let sig = registry.function_by_name("f").unwrap();
    assert_eq!(
        format_signature("f", sig),
        "f(int $x, string ...$rest): bool"
    );
}
