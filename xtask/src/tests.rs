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

/// A directory of its own under the system's temporary one, emptied.
fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_progress_view_shows_counts_runs_under_way_and_failures() {
    use super::progress::{Group, Progress, render, status_path};
    let dir = scratch_dir("progress");
    let path = status_path(&dir, "dumps");
    let group = |label: &str, total, indent| Group {
        label: label.to_string(),
        total,
        indent,
    };
    let mut progress = Progress::new(
        path.clone(),
        "dumps",
        vec![
            group("all runs", 4, 0),
            group("replays", 2, 0),
            group("one", 2, 1),
        ],
    );
    progress.start("dump a at default threads");
    progress.start("dump b at 1 threads");
    progress.finish("dump a at default threads", &[0, 1], None);
    progress.finish(
        "dump b at 1 threads",
        &[0, 2],
        Some("dump b: exit 1".into()),
    );
    progress.start("dump c at default threads");
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (view, over) = render(&path, started + 75).unwrap();
    assert!(!over, "the run is under way: {view}");
    assert!(
        view.contains("all runs  ███████████████░░░░░░░░░░░░░░░   2/4"),
        "{view}"
    );
    assert!(
        view.contains("  one     "),
        "a nested group is indented: {view}"
    );
    assert!(view.contains("running now (1)"), "{view}");
    assert!(
        view.contains("    dump c at default threads    1m 15s\n"),
        "{view}"
    );
    assert!(view.contains("failures (1)\n    dump b: exit 1"), "{view}");

    progress.end(&Err("dump b: exit 1".into()));
    let (view, over) = render(&path, started + 80).unwrap();
    assert!(over);
    assert!(!view.contains("running now"), "{view}");
    assert!(view.contains("finished: failed"), "{view}");
    std::fs::remove_dir_all(&dir).unwrap();
}
