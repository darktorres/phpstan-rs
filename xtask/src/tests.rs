use super::{manifest_profile, parse_peak_rss, test_binary_label, touch, unified_diff};

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

/// Whether the file at the components `parts` below the package's `src` is
/// engine source, the path built with the platform's separator.
fn is_engine(parts: &[&str]) -> bool {
    let root = std::path::Path::new("repo");
    let mut path = root.join("crates").join("phpstan-rs").join("src");
    path.extend(parts);
    crate::arch::is_engine_source(root, &path)
}

#[test]
fn engine_source_is_told_from_the_rest_by_the_names_of_its_path() {
    assert!(is_engine(&["checker", "includes.rs"]));
    assert!(is_engine(&["project.rs"]));
    assert!(is_engine(&["project", "walk.rs"]));
    assert!(is_engine(&["main_like.rs"]));
    assert!(!is_engine(&["project", "session.rs"]));
    assert!(!is_engine(&["main.rs"]));
    assert!(!is_engine(&["main", "check.rs"]));
    assert!(!is_engine(&["lsp.rs"]));
    assert!(!is_engine(&["lsp", "workspace_config.rs"]));
    assert!(!is_engine(&["tests.rs"]));
    assert!(!is_engine(&["checker", "tests", "includes.rs"]));
    assert!(!is_engine(&["project", "tests.rs"]));
    let root = std::path::Path::new("repo");
    assert!(!crate::arch::is_engine_source(
        root,
        &root
            .join("crates")
            .join("composer")
            .join("src")
            .join("lib.rs")
    ));
}

#[cfg(windows)]
#[test]
fn engine_source_is_told_from_the_rest_on_windows_paths() {
    let root = std::path::Path::new(r"C:\repo");
    let is = |rel: &str| crate::arch::is_engine_source(root, &root.join(rel));
    assert!(is(r"crates\phpstan-rs\src\checker\includes.rs"));
    assert!(!is(r"crates\phpstan-rs\src\project\session.rs"));
    assert!(!is(r"crates\phpstan-rs\src\main\check.rs"));
    assert!(!is(r"crates\phpstan-rs\src\lsp\workspace_config.rs"));
    assert!(!is(r"crates\phpstan-rs\src\checker\tests\includes.rs"));
    assert!(!is(r"crates\phpstan-rs\src\project\tests.rs"));
}

#[test]
fn the_peak_rss_is_the_last_line_of_gnu_times_report_in_kb() {
    assert_eq!(parse_peak_rss("123456\n", false), Some(123_456));
    assert_eq!(parse_peak_rss("a warning\n654321\n", false), Some(654_321));
    assert_eq!(parse_peak_rss("not a number\n", false), None);
    assert_eq!(parse_peak_rss("", false), None);
}

#[test]
fn the_peak_rss_is_the_bytes_bsd_time_reports_for_the_maximum_resident_set() {
    let report = "        1.23 real         0.50 user         0.10 sys\n\
                  \x20   2097152  maximum resident set size\n\
                  \x20         0  average shared memory size\n";
    assert_eq!(parse_peak_rss(report, true), Some(2048));
    assert_eq!(parse_peak_rss("1.00 real\n", true), None);
}

#[test]
fn a_unified_diff_labels_its_sides_and_keeps_three_lines_of_context() {
    let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\n";
    let new = "a\nb\nc\nd\nE\nf\ng\nh\ni\n";
    assert_eq!(
        unified_diff(old.as_bytes(), new.as_bytes()),
        "--- old\n+++ new\n@@ -2,7 +2,7 @@\n b\n c\n d\n-e\n+E\n f\n g\n h\n"
    );
    assert_eq!(unified_diff(b"x\n", b"x\n"), "");
    assert_eq!(
        unified_diff(b"a\nb", b"a\nc"),
        "--- old\n+++ new\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
    );
}

#[test]
fn touching_a_file_sets_its_modification_time_to_now_and_leaves_its_contents() {
    let dir = std::env::temp_dir().join(format!("xtask-touch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let long_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    let before = std::time::SystemTime::now();
    touch(&path).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert!(
        modified >= before - std::time::Duration::from_secs(2),
        "{modified:?}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fn main() {}\n");
    assert!(touch(&dir.join("absent.rs")).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
