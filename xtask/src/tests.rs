use super::{manifest_profile, test_binary_label};

#[test]
fn a_unit_test_binary_is_named_by_its_source_and_crate() {
    assert_eq!(
        test_binary_label(
            "     Running unittests src/lib.rs (target/debug/build/phpstan-rs-composer/97919a2680060681/out/composer-97919a2680060681)"
        ),
        Some("test src/lib.rs (composer)".to_string())
    );
}

#[test]
fn an_integration_test_or_bench_binary_is_named_by_its_file() {
    assert_eq!(
        test_binary_label(
            "     Running tests/cli.rs (target/debug/build/phpstan-rs/0123abcd/out/cli-0123abcd)"
        ),
        Some("test tests/cli.rs (cli)".to_string())
    );
    assert_eq!(
        test_binary_label(
            "     Running benches/vendored_corpus.rs (target/debug/deps/vendored_corpus-0123abcd)"
        ),
        Some("test benches/vendored_corpus.rs (vendored_corpus)".to_string())
    );
}

#[test]
fn doc_tests_are_named_by_their_crate() {
    assert_eq!(
        test_binary_label("   Doc-tests phpstan_rs"),
        Some("doc-tests phpstan_rs".to_string())
    );
}

#[test]
fn the_manifest_lists_under_no_profile_or_the_one_it_names() {
    assert_eq!(manifest_profile(&[]), Ok(None));
    let args = ["--profile".to_string(), "test".to_string()];
    assert_eq!(manifest_profile(&args), Ok(Some("test")));
}

#[test]
fn the_manifest_refuses_any_other_argument() {
    assert!(manifest_profile(&["--profile".to_string()]).is_err());
    assert!(manifest_profile(&["test".to_string()]).is_err());
    let args = ["--profile".to_string(), "test".to_string(), "x".to_string()];
    assert!(manifest_profile(&args).is_err());
}

#[test]
fn other_output_is_not_a_test_binary() {
    assert_eq!(
        test_binary_label("test result: ok. 3 passed; 0 failed"),
        None
    );
    assert_eq!(test_binary_label("   Compiling xtask v0.1.0"), None);
}

/// The architecture findings for `source` as a file at `rel` under the
/// repository root `/repo`.
fn arch_findings(rel: &str, source: &str) -> Vec<&'static str> {
    use syn::visit::Visit;
    let root = std::path::Path::new("/repo");
    let mut finder = crate::arch::Finder {
        in_diagnostics: false,
        in_engine: crate::arch::is_engine_source(root, &root.join(rel)),
        found: Vec::new(),
    };
    finder.visit_file(&syn::parse_file(source).unwrap());
    finder.found.into_iter().map(|(_, what)| what).collect()
}

#[test]
fn the_engine_may_not_resolve_a_path_against_the_process() {
    let source = "fn f(p: &std::path::Path) { let _ = std::env::current_dir(); \
                  let _ = std::fs::canonicalize(p); let _ = std::path::absolute(p); \
                  let _ = p.canonicalize(); }";
    assert_eq!(
        arch_findings("crates/phpstan-rs/src/checker/includes.rs", source).len(),
        4
    );
}

#[test]
fn the_session_the_binary_the_language_server_and_tests_may() {
    let source = "fn f() { let _ = std::env::current_dir(); }";
    for rel in [
        "crates/phpstan-rs/src/project/session.rs",
        "crates/phpstan-rs/src/main/check_project.rs",
        "crates/phpstan-rs/src/lsp/workspace_config.rs",
        "crates/phpstan-rs/src/checker/phpstan_api/tests.rs",
        "crates/composer/src/manifest.rs",
    ] {
        assert!(arch_findings(rel, source).is_empty(), "{rel}");
    }
}
