//! End-to-end tests of the actual `phpstan-rs` binary's CLI surface
//! (`--check`, `--check-project`, `--incremental-bench`, and the
//! default no-args/one-file mode). main.rs's run_check/run_check_project/
//! run_incremental_bench all print straight to stdout instead of
//! returning anything testable in-process - rather than refactor
//! working CLI-glue code just to make it unit-testable, this spawns the
//! real compiled binary (via Cargo's CARGO_BIN_EXE_<name>, no extra
//! dependency needed) and asserts on its actual stdout/stderr, the same
//! way a user would actually observe it.

use std::path::{Path, PathBuf};
use std::process::Command;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "phpstan_rs_cli_test_{label}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_phpstan-rs"))
}

#[test]
fn check_reports_every_file_parsed_with_zero_failures() {
    let dir = TempDir::new("check_ok");
    dir.write("a.php", "<?php function f(): int { return 1; }");
    dir.write("b.php", "<?php function g(): int { return 2; }");

    let output = bin().arg("--check").arg(dir.path()).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("2 files checked, 0 failed"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_reports_real_diagnostics_and_a_summary_line() {
    let dir = TempDir::new("check_project");
    dir.write("bad.php", "<?php function f() { return $undefinedVar; }");

    let output = bin()
        .arg("--check-project")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("undefined variable"), "got: {stdout}");
    assert!(
        stdout.contains("1 file(s) checked, 1 diagnostic(s)"),
        "got: {stdout}"
    );
    // A real `path:line:col:` prefix, compiler-style - not the old
    // line-1 placeholder.
    assert!(
        stdout.contains("bad.php:1:29: "),
        "expected a real line:col prefix, got: {stdout}"
    );
}

#[test]
fn check_project_reports_diagnostics_in_source_order_across_files() {
    // Two files, each with one diagnostic, loaded in an order that
    // doesn't match their sorted-by-path report order - `--check-project`
    // sorts files by path before checking (see `run_check_project`'s own
    // doc comment on determinism), so this also exercises that a real
    // per-diagnostic position doesn't disturb the existing file-level
    // sort.
    let dir = TempDir::new("check_project_order");
    dir.write(
        "z_first.php",
        "<?php\nfunction f() {\n    return $bogus;\n}\n",
    );
    dir.write(
        "a_second.php",
        "<?php\nfunction g() {\n    return $alsoBogus;\n}\n",
    );

    let output = bin()
        .arg("--check-project")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let a_pos = stdout.find("a_second.php:3:12:").unwrap_or_else(|| {
        panic!("expected a real line:col prefix for a_second.php, got: {stdout}")
    });
    let z_pos = stdout.find("z_first.php:3:12:").unwrap_or_else(|| {
        panic!("expected a real line:col prefix for z_first.php, got: {stdout}")
    });
    assert!(
        a_pos < z_pos,
        "expected a_second.php's diagnostic before z_first.php's (path-sorted), got: {stdout}"
    );
}

#[test]
fn check_project_level_flag_filters_out_higher_level_diagnostics() {
    let dir = TempDir::new("check_project_level");
    dir.write(
        "mixed.php",
        r#"<?php
            function addOne(int $x): int { return $x . "1"; }
            function f() { return $undefinedVar; }
        "#,
    );

    let unfiltered = bin()
        .arg("--check-project")
        .arg(dir.path())
        .output()
        .unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        unfiltered_stdout.contains("2 diagnostic(s)"),
        "got: {unfiltered_stdout}"
    );

    // Level 0 is the undefined-symbol/arity family - the return-type
    // mismatch (level 3) should be filtered out, leaving only the
    // undefined variable.
    let filtered = bin()
        .arg("--check-project")
        .arg("--level")
        .arg("0")
        .arg(dir.path())
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    assert!(
        filtered_stdout.contains("undefined variable"),
        "got: {filtered_stdout}"
    );
    assert!(
        !filtered_stdout.contains("return type mismatch"),
        "got: {filtered_stdout}"
    );
    assert!(
        filtered_stdout.contains("1 file(s) checked, 1 diagnostic(s)"),
        "got: {filtered_stdout}"
    );
}

#[test]
fn check_project_strict_mixed_arrays_flag_opts_into_the_extra_check() {
    let dir = TempDir::new("check_project_strict");
    dir.write(
        "arr.php",
        r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            needsInts([1, "two", 3]);
        "#,
    );

    let unfiltered = bin()
        .arg("--check-project")
        .arg(dir.path())
        .output()
        .unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        unfiltered_stdout.contains("0 diagnostic(s)"),
        "expected the untyped array literal to be let through by default, got: {unfiltered_stdout}"
    );

    let strict = bin()
        .arg("--check-project")
        .arg("--strict-mixed-arrays")
        .arg(dir.path())
        .output()
        .unwrap();
    let strict_stdout = String::from_utf8_lossy(&strict.stdout);
    assert!(
        strict_stdout.contains("expects array<int>"),
        "got: {strict_stdout}"
    );
    assert!(
        strict_stdout.contains("1 file(s) checked, 1 diagnostic(s)"),
        "got: {strict_stdout}"
    );
}

#[test]
fn check_project_generate_baseline_then_baseline_suppresses_only_pre_existing_diagnostics() {
    let dir = TempDir::new("check_project_baseline");
    dir.write("bad.php", "<?php function f() { return $existingBug; }\n");
    let baseline_path = dir.path().join("baseline.json");

    let generate = bin()
        .arg("--check-project")
        .arg("--generate-baseline")
        .arg(&baseline_path)
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(generate.status.success());
    let generate_stdout = String::from_utf8_lossy(&generate.stdout);
    assert!(
        generate_stdout.contains("baseline written to"),
        "got: {generate_stdout}"
    );
    assert!(baseline_path.exists(), "expected a baseline file to exist");

    // A run with the baseline applied reports nothing for the
    // already-recorded diagnostic.
    let filtered = bin()
        .arg("--check-project")
        .arg("--baseline")
        .arg(&baseline_path)
        .arg(dir.path())
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    assert!(
        filtered_stdout.contains("1 file(s) checked, 0 diagnostic(s)"),
        "got: {filtered_stdout}"
    );

    // A run without --baseline still reports it - the baseline only
    // suppresses when explicitly opted into.
    let unfiltered = bin()
        .arg("--check-project")
        .arg(dir.path())
        .output()
        .unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        unfiltered_stdout.contains("existingBug"),
        "got: {unfiltered_stdout}"
    );

    // A genuinely new diagnostic still surfaces even with the baseline
    // applied - only the pre-existing one is suppressed.
    dir.write(
        "bad.php",
        "<?php function f() { return $existingBug; } function g() { return $newBug; }\n",
    );
    let with_new_bug = bin()
        .arg("--check-project")
        .arg("--baseline")
        .arg(&baseline_path)
        .arg(dir.path())
        .output()
        .unwrap();
    let with_new_bug_stdout = String::from_utf8_lossy(&with_new_bug.stdout);
    assert!(
        with_new_bug_stdout.contains("newBug"),
        "got: {with_new_bug_stdout}"
    );
    assert!(
        !with_new_bug_stdout.contains("existingBug"),
        "got: {with_new_bug_stdout}"
    );
    assert!(
        with_new_bug_stdout.contains("1 file(s) checked, 1 diagnostic(s)"),
        "got: {with_new_bug_stdout}"
    );
}

#[test]
fn check_project_with_no_args_prints_usage_to_stderr() {
    let output = bin().arg("--check-project").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage:"), "got: {stderr}");
}

#[test]
fn incremental_bench_reports_full_load_and_incremental_timings() {
    let dir = TempDir::new("bench");
    dir.write("a.php", "<?php function f(): int { return 1; }");
    dir.write("b.php", "<?php function g(): int { return f(); }");

    let output = bin()
        .arg("--incremental-bench")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("full load:"), "got: {stdout}");
    assert!(stdout.contains("incremental update:"), "got: {stdout}");
}

#[test]
fn incremental_bench_with_no_args_prints_usage_to_stderr() {
    let output = bin().arg("--incremental-bench").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage:"), "got: {stderr}");
}

#[test]
fn no_arguments_parses_the_built_in_sample_and_reports_no_diagnostics() {
    let output = bin().output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("(no file given, parsing built-in sample)"),
        "got: {stdout}"
    );
    assert!(stdout.contains("-- diagnostics --"), "got: {stdout}");
    assert!(stdout.contains("(none)"), "got: {stdout}");
}

#[test]
fn a_single_file_argument_parses_and_checks_that_file() {
    let dir = TempDir::new("single_file");
    let path = dir.write("sample.php", "<?php function f() { return $undefinedVar; }");

    let output = bin().arg(&path).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-- diagnostics --"), "got: {stdout}");
    assert!(stdout.contains("undefined variable"), "got: {stdout}");
}
