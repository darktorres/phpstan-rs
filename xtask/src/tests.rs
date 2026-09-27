use super::test_binary_label;

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
fn other_output_is_not_a_test_binary() {
    assert_eq!(
        test_binary_label("test result: ok. 3 passed; 0 failed"),
        None
    );
    assert_eq!(test_binary_label("   Compiling xtask v0.1.0"), None);
}
