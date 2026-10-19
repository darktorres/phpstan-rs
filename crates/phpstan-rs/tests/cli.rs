//! End-to-end tests of the actual `phpstan-rs` binary's CLI surface
//! (`check`, `check-project`, `incremental-bench`, and `demo`, the
//! explicit subcommand for running with no arguments at all).
//! main.rs's run_check/run_check_project/run_incremental_bench/run_demo
//! all print straight to stdout instead of returning anything testable
//! in-process - rather than refactor working CLI-glue code just to make
//! it unit-testable, this spawns the real compiled binary (via Cargo's
//! CARGO_BIN_EXE_<name>, no extra dependency needed) and asserts on its
//! actual stdout/stderr, the same way a user would actually observe it.

mod common;

use common::{TempDir, bin};
use std::process::Stdio;

#[test]
fn check_reports_every_file_parsed_with_zero_failures() {
    let dir = TempDir::new("check_ok");
    dir.write("a.php", "<?php function f(): int { return 1; }");
    dir.write("b.php", "<?php function g(): int { return 2; }");

    let output = bin().arg("check").arg(dir.path()).output().unwrap();
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
    dir.write(
        "bad.php",
        "<?php function f(): mixed { return $undefinedVar; }",
    );

    let output = bin().arg("check-project").arg(dir.path()).output().unwrap();
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
        stdout.contains("bad.php:1:36: "),
        "expected a real line:col prefix, got: {stdout}"
    );
}

#[test]
fn check_project_writes_the_type_dump_it_is_asked_for() {
    let dir = TempDir::new("dump_types");
    dir.write("a.php", "<?php $x = 1;\n$y = 'two';\n");
    let dump = dir.path().join("types.txt");

    let output = bin()
        .arg("check-project")
        .arg("--dump-types")
        .arg(&dump)
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = std::fs::read_to_string(&dump).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "got: {text}");
    assert!(
        lines[0].ends_with("a.php:1:7 [] x: 1 (native 1)"),
        "got: {text}"
    );
    assert!(lines[1].contains("a.php:2:1 [] "), "got: {text}");
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

    let output = bin().arg("check-project").arg(dir.path()).output().unwrap();
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
            function f(): mixed { return $undefinedVar; }
        "#,
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        unfiltered_stdout.contains("2 diagnostic(s)"),
        "got: {unfiltered_stdout}"
    );

    // Level 0 is the undefined-symbol/arity family - the return-type
    // mismatch (level 3) should be filtered out, leaving only the
    // undefined variable.
    let filtered = bin()
        .arg("check-project")
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
    // A literal array's element types are real (an ArrayShape, not
    // always Array(Mixed)), so a genuinely wrong literal element is
    // caught by ordinary, non-strict checking - what `--set rules.strict_mixed_arrays=true`
    // adds is catching a genuinely unknown (Mixed-element) array, such
    // as one read back out of an untyped parameter.
    let dir = TempDir::new("check_project_strict");
    dir.write(
        "arr.php",
        r#"<?php
            /** @param int[] $xs */
            // A non-empty, impure body - the transitive-purity pass would
            // otherwise prove an empty-bodied `needsInts` effect-free,
            // making its statement-level call below a second, unrelated
            // `function.resultUnused` finding that has nothing to do
            // with this test's actual point.
            function needsInts(array $xs): void { echo count($xs); }
            function test(array $untyped): void { needsInts($untyped); }
        "#,
    );

    // `--level 5` here only keeps this assertion focused on
    // `strict_mixed_arrays`'s own point (whether an unknown-element
    // array is reported by default) - it doesn't change whether the
    // check itself runs (that's gated by `--set rules.strict_mixed_arrays=true` alone,
    // not by level), just excludes the unrelated level-6
    // `missingType.iterableValue` finding on `$untyped`'s deliberately-bare
    // `array` parameter, which this test needs to stay bare to exercise
    // "unknown element type" at all.
    let unfiltered = bin()
        .arg("check-project")
        .arg("--level")
        .arg("5")
        .arg(dir.path())
        .output()
        .unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        unfiltered_stdout.contains("0 diagnostic(s)"),
        "expected the untyped array to be let through by default, got: {unfiltered_stdout}"
    );

    let strict = bin()
        .arg("check-project")
        .args(["--set", "rules.strict_mixed_arrays=true"])
        .arg("--level")
        .arg("5")
        .arg(dir.path())
        .output()
        .unwrap();
    let strict_stdout = String::from_utf8_lossy(&strict.stdout);
    assert!(
        strict_stdout.contains("expects array<(int|string), int>"),
        "got: {strict_stdout}"
    );
    assert!(
        strict_stdout.contains("1 file(s) checked, 1 diagnostic(s)"),
        "got: {strict_stdout}"
    );
}

#[test]
fn check_project_check_uninitialized_properties_flag_opts_into_the_extra_check() {
    // Off by default (mirroring PHPStan's own opt-in
    // `checkUninitializedProperties`, see `RulesSettings::
    // check_uninitialized_properties`'s own doc comment) - only
    // `--set rules.check_uninitialized_properties=true` turns it on.
    let dir = TempDir::new("check_project_uninit");
    dir.write(
        "foo.php",
        r#"<?php
            class Foo {
                public int $x;
            }
        "#,
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        !unfiltered_stdout.contains("property.uninitialized"),
        "expected the check to be off by default, got: {unfiltered_stdout}"
    );

    let enabled = bin()
        .arg("check-project")
        .args(["--set", "rules.check_uninitialized_properties=true"])
        .arg(dir.path())
        .output()
        .unwrap();
    let enabled_stdout = String::from_utf8_lossy(&enabled.stdout);
    assert!(
        enabled_stdout.contains("property.uninitialized"),
        "got: {enabled_stdout}"
    );
}

#[test]
fn check_project_check_dead_enum_cases_flag_opts_into_the_extra_check() {
    // Off by default (see `RulesSettings::check_dead_enum_cases`'s
    // own doc comment) - only `--set rules.check_dead_enum_cases=true` turns it on.
    // Also exercises the real end-to-end path: `check_whole_program`
    // wired into `check-project`'s own primary loop, not just the
    // library-level `Project::check_whole_program` unit tests.
    let dir = TempDir::new("check_project_dead_enum");
    dir.write(
        "foo.php",
        r#"<?php
            enum Suit {
                case Hearts;
                case Spades;
            }
        "#,
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        !unfiltered_stdout.contains("enumCase.unused"),
        "expected the check to be off by default, got: {unfiltered_stdout}"
    );

    let enabled = bin()
        .arg("check-project")
        .args(["--set", "rules.check_dead_enum_cases=true"])
        .arg(dir.path())
        .output()
        .unwrap();
    let enabled_stdout = String::from_utf8_lossy(&enabled.stdout);
    assert!(
        enabled_stdout.contains("enumCase.unused") && enabled_stdout.contains("Suit::Hearts"),
        "got: {enabled_stdout}"
    );
}

#[test]
fn check_project_check_dead_code_flag_opts_into_the_extra_check() {
    // Off by default (see `RulesSettings::check_dead_code`'s own
    // doc comment) - only `--set rules.check_dead_code=true` turns it on. Also
    // exercises the real end-to-end path: `check_whole_program` wired
    // into `check-project`'s own primary loop, not just the
    // library-level `Project::check_whole_program` unit tests.
    let dir = TempDir::new("check_project_dead_code");
    dir.write(
        "foo.php",
        r#"<?php
            class UnusedClass {}
            function unusedFunction() {}
        "#,
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        !unfiltered_stdout.contains("class.unused")
            && !unfiltered_stdout.contains("function.unused"),
        "expected the check to be off by default, got: {unfiltered_stdout}"
    );

    let enabled = bin()
        .arg("check-project")
        .args(["--set", "rules.check_dead_code=true"])
        .arg(dir.path())
        .output()
        .unwrap();
    let enabled_stdout = String::from_utf8_lossy(&enabled.stdout);
    assert!(
        enabled_stdout.contains("class.unused") && enabled_stdout.contains("UnusedClass"),
        "got: {enabled_stdout}"
    );
    assert!(
        enabled_stdout.contains("function.unused") && enabled_stdout.contains("unusedFunction"),
        "got: {enabled_stdout}"
    );
}

#[test]
fn check_project_check_circular_dependencies_flag_opts_into_the_extra_check() {
    // Off by default (see `RulesSettings::check_circular_
    // dependencies`'s own doc comment) - only
    // `--set rules.check_circular_dependencies=true` turns it on. Two different class
    // pairs, each edge one-directional, so no single pair has a
    // (necessarily-invalid-PHP) class-level cycle - only NsA and NsB
    // depend on each other in aggregate.
    let dir = TempDir::new("check_project_circular_deps");
    dir.write(
        "a.php",
        r#"<?php
            namespace NsA;
            class A extends \NsB\B {}
            class D {}
        "#,
    );
    dir.write(
        "b.php",
        r#"<?php
            namespace NsB;
            class B {}
            class C extends \NsA\D {}
        "#,
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        !unfiltered_stdout.contains("namespace.circularDependency"),
        "expected the check to be off by default, got: {unfiltered_stdout}"
    );

    let enabled = bin()
        .arg("check-project")
        .args(["--set", "rules.check_circular_dependencies=true"])
        .arg(dir.path())
        .output()
        .unwrap();
    let enabled_stdout = String::from_utf8_lossy(&enabled.stdout);
    assert!(
        enabled_stdout.contains("namespace.circularDependency")
            && enabled_stdout.contains("NsA")
            && enabled_stdout.contains("NsB"),
        "got: {enabled_stdout}"
    );
}

#[test]
fn check_project_check_dead_abstract_classes_flag_opts_into_the_extra_check() {
    // Off by default (see `RulesSettings::check_dead_abstract_
    // classes`'s own doc comment) - only `--set rules.check_dead_abstract_classes=true`
    // turns it on.
    let dir = TempDir::new("check_project_dead_abstract");
    dir.write(
        "foo.php",
        "<?php\n            abstract class DeadBase {}\n        ",
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        !unfiltered_stdout.contains("class.deadAbstract"),
        "expected the check to be off by default, got: {unfiltered_stdout}"
    );

    let enabled = bin()
        .arg("check-project")
        .args(["--set", "rules.check_dead_abstract_classes=true"])
        .arg(dir.path())
        .output()
        .unwrap();
    let enabled_stdout = String::from_utf8_lossy(&enabled.stdout);
    assert!(
        enabled_stdout.contains("class.deadAbstract") && enabled_stdout.contains("DeadBase"),
        "got: {enabled_stdout}"
    );
}

#[test]
fn check_project_check_dead_members_flag_opts_into_the_extra_check() {
    // Off by default (see `RulesSettings::check_dead_members`'s
    // own doc comment) - only `--set rules.check_dead_members=true` turns it on.
    let dir = TempDir::new("check_project_dead_members");
    dir.write(
        "foo.php",
        r#"<?php
            class Foo {
                public function unusedMethod(): void {}
            }
        "#,
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        !unfiltered_stdout.contains("method.unused"),
        "expected the check to be off by default, got: {unfiltered_stdout}"
    );

    let enabled = bin()
        .arg("check-project")
        .args(["--set", "rules.check_dead_members=true"])
        .arg(dir.path())
        .output()
        .unwrap();
    let enabled_stdout = String::from_utf8_lossy(&enabled.stdout);
    assert!(
        enabled_stdout.contains("method.unused") && enabled_stdout.contains("Foo::unusedMethod"),
        "got: {enabled_stdout}"
    );
}

#[test]
fn check_project_check_internal_usage_flag_opts_into_the_extra_check() {
    // Off by default (mirroring PHPStan's own opt-in
    // `featureToggles.internalTag`, see `RulesSettings::
    // check_internal_usage`'s own doc comment) - only
    // `--set rules.check_internal_usage=true` turns it on.
    let dir = TempDir::new("check_project_internal_usage");
    dir.write(
        "foo.php",
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            function make(): void {}

            namespace Other\App;
            function f() {
                \Vendor\Internal\make();
            }
        "#,
    );

    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        !unfiltered_stdout.contains("function.internal"),
        "expected the check to be off by default, got: {unfiltered_stdout}"
    );

    let enabled = bin()
        .arg("check-project")
        .args(["--set", "rules.check_internal_usage=true"])
        .arg(dir.path())
        .output()
        .unwrap();
    let enabled_stdout = String::from_utf8_lossy(&enabled.stdout);
    assert!(
        enabled_stdout.contains("function.internal"),
        "got: {enabled_stdout}"
    );
}

#[test]
fn check_project_a_wrong_typed_array_literal_is_flagged_without_strict_mixed_arrays() {
    // `strict_mixed_arrays` isn't necessary to catch this, since array
    // literals carry a real per-element type.
    let dir = TempDir::new("check_project_literal_shape");
    dir.write(
        "arr.php",
        r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            needsInts([1, "two", 3]);
        "#,
    );

    let output = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("expects array<(int|string), int>"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_php_version_flag_is_accepted_and_checking_proceeds_normally() {
    // Confirms the flag parses and a normal run still completes and
    // reports as usual.
    let dir = TempDir::new("check_project_php_version");
    dir.write("a.php", "<?php function f(): int { return 1; }");

    let output = bin()
        .arg("check-project")
        .arg("--php-version")
        .arg("8.1")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 file(s) checked, 0 diagnostic(s)"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_malformed_php_version_is_a_real_error_not_a_panic() {
    let dir = TempDir::new("check_project_bad_php_version");
    dir.write("a.php", "<?php function f(): int { return 1; }");

    let output = bin()
        .arg("check-project")
        .arg("--php-version")
        .arg("not-a-version")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid php_version") && stderr.contains("not-a-version"),
        "got: {stderr}"
    );
}

#[test]
fn check_project_config_file_supplies_php_version() {
    // CLI-level mirror of `strict_mixed_arrays`'s own config-supplies-a-
    // value coverage: a config file's `php_version` key is honored (and,
    // being unobservable in diagnostics today, is exercised here only via
    // the malformed-value error path, which does surface on stderr).
    let dir = TempDir::new("check_project_config_php_version");
    dir.write("src/a.php", "<?php function f(): int { return 1; }");
    let config_path = dir.write(
        "phpstan-rs.toml",
        "paths = [\"src\"]\nphp_version = \"not-a-version\"\n",
    );

    let output = bin()
        .arg("check-project")
        .arg("--config")
        .arg(&config_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid php_version") && stderr.contains("not-a-version"),
        "got: {stderr}"
    );
}

#[test]
fn check_project_cli_php_version_overrides_config_php_version() {
    // Same override precedent as `strict_mixed_arrays`: a CLI flag beats
    // the same key in the config file. A valid CLI value must win over an
    // invalid config value (if the config value were used instead, this
    // would fail the same way the test above does).
    let dir = TempDir::new("check_project_config_php_version_override");
    dir.write("src/a.php", "<?php function f(): int { return 1; }");
    let config_path = dir.write(
        "phpstan-rs.toml",
        "paths = [\"src\"]\nphp_version = \"not-a-version\"\n",
    );

    let output = bin()
        .arg("check-project")
        .arg("--config")
        .arg(&config_path)
        .arg("--php-version")
        .arg("8.2")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 file(s) checked, 0 diagnostic(s)"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_config_file_supplies_paths_and_level() {
    let dir = TempDir::new("check_project_config");
    dir.write(
        "mixed.php",
        r#"<?php
            function addOne(int $x): int { return $x . "1"; }
            function f() { return $undefinedVar; }
        "#,
    );
    let config_path = dir.write(
        "phpstan.toml",
        &format!(
            "paths = [{:?}]\nlevel = 0\n",
            dir.path().join("mixed.php").display().to_string()
        ),
    );

    // No positional dirs given at all - the config file supplies both
    // which file to check and which level to report.
    let output = bin()
        .arg("check-project")
        .arg("--config")
        .arg(&config_path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("undefined variable"), "got: {stdout}");
    assert!(
        !stdout.contains("return type mismatch"),
        "expected the config's level = 0 to filter out the level-3 finding, got: {stdout}"
    );
}

#[test]
fn check_project_config_relative_paths_resolve_against_the_config_file_not_the_cwd() {
    // A config file's relative `paths` must resolve against the config
    // file's own directory, matching every comparable format
    // (.gitignore, tsconfig.json, PHPStan's own neon includes) - not
    // whatever directory happens to invoke the CLI.
    // Run with `current_dir` set somewhere else entirely (the system
    // temp root, guaranteed to hold no `src/` of its own) so a bug that
    // resolves "src" against the process's cwd instead would report "no
    // .php files found" rather than actually checking the right file.
    let dir = TempDir::new("check_project_config_relative");
    dir.write(
        "src/mixed.php",
        r#"<?php
            function f() { return $undefinedVar; }
        "#,
    );
    let config_path = dir.write("phpstan.toml", "paths = [\"src\"]\n");

    let output = bin()
        .arg("check-project")
        .arg("--config")
        .arg(&config_path)
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("undefined variable"),
        "expected the config's relative \"src\" path to resolve against the config file's own \
         directory, got stdout: {stdout} / stderr: {stderr}"
    );
}

#[test]
fn check_project_cli_level_overrides_a_conflicting_config_level() {
    let dir = TempDir::new("check_project_config_override");
    dir.write(
        "mixed.php",
        r#"<?php
            function addOne(int $x): int { return $x . "1"; }
            function f() { return $undefinedVar; }
        "#,
    );
    let config_path = dir.write("phpstan.toml", "level = 0\n");

    let output = bin()
        .arg("check-project")
        .arg("--config")
        .arg(&config_path)
        .arg("--level")
        .arg("6")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("return type mismatch"),
        "expected the explicit CLI --level 6 to override the config's level = 0, got: {stdout}"
    );
}

#[test]
fn check_project_generate_baseline_then_baseline_suppresses_only_pre_existing_diagnostics() {
    let dir = TempDir::new("check_project_baseline");
    dir.write("bad.php", "<?php function f() { return $existingBug; }\n");
    let baseline_path = dir.path().join("baseline.json");

    let generate = bin()
        .arg("check-project")
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
        .arg("check-project")
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
    let unfiltered = bin().arg("check-project").arg(dir.path()).output().unwrap();
    let unfiltered_stdout = String::from_utf8_lossy(&unfiltered.stdout);
    assert!(
        unfiltered_stdout.contains("existingBug"),
        "got: {unfiltered_stdout}"
    );

    // A genuinely new diagnostic still surfaces even with the baseline
    // applied - only the pre-existing one is suppressed.
    dir.write(
        "bad.php",
        "<?php function f() { return $existingBug; } function g(): mixed { return $newBug; }\n",
    );
    let with_new_bug = bin()
        .arg("check-project")
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
fn check_project_baseline_still_matches_a_differently_spelled_but_identical_path() {
    // Generating with "." then checking with "./." (the same directory,
    // spelled differently) must not make the whole baseline reappear as
    // new - a realistic mismatch (a one-off local command vs. a CI
    // script, a Makefile target vs. a manual run).
    let dir = TempDir::new("check_project_baseline_path_spelling");
    dir.write("bad.php", "<?php function f() { return $existingBug; }\n");
    let baseline_path = dir.path().join("baseline.json");

    let generate = bin()
        .arg("check-project")
        .arg(".")
        .arg("--generate-baseline")
        .arg(&baseline_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(generate.status.success());

    let filtered = bin()
        .arg("check-project")
        .arg("./.")
        .arg("--baseline")
        .arg(&baseline_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    assert!(
        filtered_stdout.contains("1 file(s) checked, 0 diagnostic(s)"),
        "expected the baseline to still match despite the differently-spelled path, got: {filtered_stdout}"
    );
}

#[test]
fn check_project_with_no_args_and_no_config_prints_an_error_to_stderr() {
    // `dirs` isn't `required = true` on the clap side (a config file's
    // own `paths` can supply it instead - see Part B's own resolution
    // logic in `main.rs`), so this is a real, hand-written error now,
    // not a clap-generated one.
    let output = bin().arg("check-project").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no paths given on the command line or in the config file"),
        "got: {stderr}"
    );
}

#[test]
fn check_project_scan_dir_loads_context_but_never_reports_on_it() {
    let dir = TempDir::new("scan_dir_context");
    // A genuinely buggy file that must never be reported on.
    dir.write(
        "ctx/Helper.php",
        "<?php class Helper { function bad() { return $undefinedInVendor; } }",
    );
    dir.write("src/Main.php", "<?php function f(): void { new Helper(); }");

    // Without --scan-dir, Helper is unresolved.
    let without = bin()
        .arg("check-project")
        .arg(dir.path().join("src"))
        .output()
        .unwrap();
    let without_stdout = String::from_utf8_lossy(&without.stdout);
    assert!(
        without_stdout.contains("not found"),
        "got: {without_stdout}"
    );

    // With --scan-dir, Helper resolves, but its own bug is never
    // reported and the summary reflects it as context, not "checked".
    let with = bin()
        .arg("check-project")
        .arg(dir.path().join("src"))
        .arg("--scan-dir")
        .arg(dir.path().join("ctx"))
        .output()
        .unwrap();
    assert!(with.status.success());
    let with_stdout = String::from_utf8_lossy(&with.stdout);
    assert!(!with_stdout.contains("not found"), "got: {with_stdout}");
    assert!(
        !with_stdout.contains("undefinedInVendor"),
        "got: {with_stdout}"
    );
    assert!(
        with_stdout.contains("1 file(s) checked, 0 diagnostic(s)"),
        "got: {with_stdout}"
    );
    assert!(
        with_stdout.contains("1 more file(s) loaded for context"),
        "got: {with_stdout}"
    );
}

#[test]
fn check_project_scan_paths_config_mirrors_scan_dir() {
    let dir = TempDir::new("scan_paths_config");
    dir.write(
        "ctx/Helper.php",
        "<?php class Helper { function bad() { return $undefinedInVendor; } }",
    );
    dir.write("src/Main.php", "<?php function f(): void { new Helper(); }");
    let config_path = dir.write(
        "phpstan.toml",
        &format!(
            "paths = [{:?}]\nscan_paths = [{:?}]\n",
            dir.path().join("src").display().to_string(),
            dir.path().join("ctx").display().to_string()
        ),
    );

    let output = bin()
        .arg("check-project")
        .arg("--config")
        .arg(&config_path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("not found"), "got: {stdout}");
    assert!(
        stdout.contains("1 file(s) checked, 0 diagnostic(s)"),
        "got: {stdout}"
    );
    assert!(
        stdout.contains("1 more file(s) loaded for context"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_auto_detects_composer_vendor_and_never_reports_on_it() {
    let dir = TempDir::new("composer_auto_detect");
    dir.write("composer.json", "{}");
    dir.write(
        "vendor/acme/Helper.php",
        "<?php class Helper { function bad() { return $undefinedInVendor; } }",
    );
    dir.write(
        "vendor/composer/autoload_real.php",
        "<?php\n// autoload_real.php @generated by Composer\n",
    );
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Helper' => $vendorDir . '/acme/Helper.php',
);
"#,
    );
    dir.write("src/Main.php", "<?php function f(): void { new Helper(); }");

    let output = bin()
        .current_dir(dir.path())
        .arg("check-project")
        .arg(dir.path().join("src"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("not found"), "got: {stdout}");
    assert!(!stdout.contains("undefinedInVendor"), "got: {stdout}");
    assert!(
        stdout.contains("1 file(s) checked, 0 diagnostic(s)"),
        "got: {stdout}"
    );
    assert!(
        stdout.contains("1 more file(s) loaded for context"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_no_composer_flag_disables_auto_detection() {
    let dir = TempDir::new("no_composer_flag");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/Helper.php", "<?php class Helper {}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Helper' => $vendorDir . '/acme/Helper.php',
);
"#,
    );
    dir.write("src/Main.php", "<?php function f(): void { new Helper(); }");

    let output = bin()
        .current_dir(dir.path())
        .arg("check-project")
        .arg(dir.path().join("src"))
        .arg("--no-composer")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not found"), "got: {stdout}");
    assert!(
        !stdout.contains("more file(s) loaded for context"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_no_composer_config_disables_auto_detection_without_the_flag() {
    let dir = TempDir::new("no_composer_config");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/Helper.php", "<?php class Helper {}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Helper' => $vendorDir . '/acme/Helper.php',
);
"#,
    );
    dir.write("src/Main.php", "<?php function f(): void { new Helper(); }");
    let config_path = dir.write(
        "phpstan.toml",
        &format!(
            "paths = [{:?}]\nno_composer = true\n",
            dir.path().join("src").display().to_string()
        ),
    );

    // No --no-composer flag at all - the config alone must suppress
    // auto-detection, since a real project will want to set this once
    // rather than remember a flag on every invocation.
    let output = bin()
        .current_dir(dir.path())
        .arg("check-project")
        .arg("--config")
        .arg(&config_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not found"), "got: {stdout}");
    assert!(
        !stdout.contains("more file(s) loaded for context"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_context_file_never_wins_a_fqcn_collision_over_an_analyzed_file() {
    // A context (vendor) file and an analyzed (first-party) file both
    // declare a class named `Shape`, with a distinguishably different
    // `area()` return type. If load order ever regresses (context
    // loaded after analyze instead of before), the registry's `Shape`
    // entry would end up as the vendor stand-in's `int`-returning
    // version, and Caller.php's use of it as a `string` would produce a
    // real return-type-mismatch diagnostic that must never appear.
    let dir = TempDir::new("fqcn_collision");
    dir.write(
        "ctx/Shape.php",
        "<?php class Shape { function area(): int { return 1; } }",
    );
    dir.write(
        "src/Shape.php",
        r#"<?php class Shape { function area(): string { return "hi"; } }"#,
    );
    dir.write(
        "src/Caller.php",
        r#"<?php function f(): string { return (new Shape())->area(); }"#,
    );

    let output = bin()
        .arg("check-project")
        .arg(dir.path().join("src"))
        .arg("--scan-dir")
        .arg(dir.path().join("ctx"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("return type mismatch"),
        "expected the analyzed Shape::area()'s own `string` return type to win the \
         registry slot over the context file's `int` version, got: {stdout}"
    );
    assert!(
        stdout.contains("2 file(s) checked, 0 diagnostic(s)"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_survives_a_file_that_crashes_the_parser() {
    // Reproduces the same "expected Semicolon, found LParen" panic
    // shape a real, unmodified vendor package (dompdf/dompdf) is
    // confirmed to trip in this parser - a small synthetic snippet
    // rather than depending on that package's current state.
    let dir = TempDir::new("panic_containment");
    dir.write(
        "bad.php",
        "<?php class Foo { public readonly int $x (int $y); }",
    );
    dir.write(
        "good.php",
        "<?php function f(): mixed { return $undefinedVar; }",
    );

    let output = bin().arg("check-project").arg(dir.path()).output().unwrap();
    assert!(
        output.status.success(),
        "expected check-project to survive a file that crashes the parser, \
         stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("undefined variable"), "got: {stdout}");
    // The file that does not parse is reported as a `parse.error`, and
    // counted among the files with findings.
    assert!(stdout.contains("(parse.error)"), "got: {stdout}");
    assert!(
        stdout.contains("1 file(s) checked, 2 diagnostic(s)"),
        "got: {stdout}"
    );
    assert!(
        stdout.contains("1 file(s) skipped: parse error"),
        "got: {stdout}"
    );
}

#[test]
fn check_project_format_json_emits_a_parseable_report_with_no_extra_stdout_noise() {
    let dir = TempDir::new("format_json");
    dir.write(
        "bad.php",
        "<?php function f(): mixed { return $undefinedVar; }",
    );

    let output = bin()
        .arg("check-project")
        .arg("--format")
        .arg("json")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The whole of stdout must be one JSON document - a CI pipeline piping
    // this straight into a JSON parser must never see the old text
    // format's prose mixed in.
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("expected valid JSON on stdout, got {e}: {stdout}"));
    assert_eq!(parsed["totals"]["files_checked"], 1);
    assert_eq!(parsed["totals"]["diagnostics"], 1);
    let bad_php_key = parsed["files"]
        .as_object()
        .unwrap()
        .keys()
        .find(|k| k.ends_with("bad.php"))
        .unwrap_or_else(|| panic!("expected a bad.php entry in files, got: {stdout}"))
        .clone();
    let file = &parsed["files"][&bad_php_key];
    assert_eq!(file["diagnostics"], 1);
    let message = file["messages"][0]["message"].as_str().unwrap();
    assert!(message.contains("undefined variable"), "got: {message}");
    assert_eq!(file["messages"][0]["line"], 1);
}

#[test]
fn check_project_format_json_respects_the_level_filter() {
    let dir = TempDir::new("format_json_level");
    dir.write(
        "mixed.php",
        r#"<?php
            function addOne(int $x): int { return $x . "1"; }
            function f() { return $undefinedVar; }
        "#,
    );

    let output = bin()
        .arg("check-project")
        .arg("--format")
        .arg("json")
        .arg("--level")
        .arg("0")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["totals"]["diagnostics"], 1);
}

#[test]
fn check_project_format_json_on_a_clean_project_reports_zero_diagnostics_and_no_files_entry() {
    let dir = TempDir::new("format_json_clean");
    dir.write("good.php", "<?php function f(): int { return 1; }");

    let output = bin()
        .arg("check-project")
        .arg("--format")
        .arg("json")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["totals"]["files_checked"], 1);
    assert_eq!(parsed["totals"]["diagnostics"], 0);
    assert_eq!(parsed["files"].as_object().unwrap().len(), 0);
}

#[test]
fn check_project_format_checkstyle_emits_well_formed_xml_on_stdout() {
    let dir = TempDir::new("format_checkstyle");
    dir.write(
        "bad.php",
        "<?php function f(): mixed { return $undefinedVar; }",
    );

    let output = bin()
        .arg("check-project")
        .arg("--format")
        .arg("checkstyle")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "got: {stdout}"
    );
    assert!(stdout.contains("<checkstyle"), "got: {stdout}");
    assert!(stdout.contains("bad.php"), "got: {stdout}");
    assert!(stdout.contains("severity=\"error\""), "got: {stdout}");
    assert!(stdout.contains("undefined variable"), "got: {stdout}");
    assert!(
        stdout.trim_end().ends_with("</checkstyle>"),
        "got: {stdout}"
    );
    // The old text format's own summary prose must not leak into this
    // format's stdout.
    assert!(!stdout.contains("file(s) checked"), "got: {stdout}");
}

#[test]
fn check_project_format_checkstyle_on_a_clean_project_has_no_file_elements() {
    let dir = TempDir::new("format_checkstyle_clean");
    dir.write("good.php", "<?php function f(): int { return 1; }");

    let output = bin()
        .arg("check-project")
        .arg("--format")
        .arg("checkstyle")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("<file"), "got: {stdout}");
    assert!(stdout.contains("<checkstyle"), "got: {stdout}");
}

#[test]
fn check_project_no_format_flag_still_uses_the_original_text_format() {
    let dir = TempDir::new("format_default");
    dir.write(
        "bad.php",
        "<?php function f(): mixed { return $undefinedVar; }",
    );

    let output = bin().arg("check-project").arg(dir.path()).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 file(s) checked, 1 diagnostic(s)"),
        "got: {stdout}"
    );
    assert!(!stdout.trim_start().starts_with('{'), "got: {stdout}");
}

#[test]
fn check_project_format_rejects_an_unknown_value() {
    let dir = TempDir::new("format_invalid");
    dir.write("good.php", "<?php function f(): int { return 1; }");

    let output = bin()
        .arg("check-project")
        .arg("--format")
        .arg("yaml")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("yaml"), "got: {stderr}");
}

#[test]
fn check_project_ends_quietly_with_status_141_when_its_stdout_is_closed() {
    let dir = TempDir::new("check_project_closed_stdout");
    dir.write(
        "bad.php",
        "<?php function f(): mixed { return $undefinedVar; }",
    );

    for format in ["text", "friendly", "json", "checkstyle", "junit"] {
        let mut child = bin()
            .arg("check-project")
            .arg(dir.path())
            .args(["--format", format])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The reader closes before the report is written, so the report's
        // write meets a closed pipe.
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(141), "{format}: {stderr}");
        assert!(!stderr.contains("panicked"), "{format}: {stderr}");
    }
}

#[test]
fn incremental_bench_reports_full_load_and_incremental_timings() {
    let dir = TempDir::new("bench");
    dir.write("a.php", "<?php function f(): int { return 1; }");
    dir.write("b.php", "<?php function g(): int { return f(); }");

    let output = bin()
        .arg("incremental-bench")
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
    let output = bin().arg("incremental-bench").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.to_lowercase().contains("usage:"), "got: {stderr}");
}

#[test]
fn bare_invocation_with_no_subcommand_prints_help_and_exits_nonzero() {
    // Real, idiomatic clap behavior (`arg_required_else_help`) -
    // replaces the old silent default of running the demo/sample mode
    // with zero arguments at all.
    let output = bin().output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage:"), "got: {stderr}");
    assert!(stderr.contains("demo"), "got: {stderr}");
}

#[test]
fn demo_with_no_file_parses_the_built_in_sample_and_reports_no_diagnostics() {
    let output = bin().arg("demo").output().unwrap();
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
fn demo_with_a_file_argument_parses_and_checks_that_file() {
    let dir = TempDir::new("single_file");
    let path = dir.write(
        "sample.php",
        "<?php function f(): mixed { return $undefinedVar; }",
    );

    let output = bin().arg("demo").arg(&path).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-- diagnostics --"), "got: {stdout}");
    assert!(stdout.contains("undefined variable"), "got: {stdout}");
}

#[test]
fn check_does_not_crash_on_deep_nesting_or_long_chains() {
    // Nesting and flat chains alike build deep trees; every walk over them
    // grows its stack on demand, so none may abort the process.
    let dir = TempDir::new("deep_nesting");
    let depth = 3_000;
    dir.write(
        "parens.php",
        &format!("<?php $x = {}1{};", "(".repeat(depth), ")".repeat(depth)),
    );
    dir.write(
        "concat.php",
        &format!(
            "<?php function f(string $s): string {{ return $s{}; }}",
            " . $s".repeat(depth)
        ),
    );
    dir.write(
        "elseif.php",
        &format!(
            "<?php function f(int $x): int {{ if ($x === 0) {{ return 0; }} {} return -1; }}",
            (1..depth)
                .map(|i| format!("elseif ($x === {i}) {{ return {i}; }}"))
                .collect::<String>()
        ),
    );

    // A finished check exits 0. A crash ends in a signal on Unix, which has
    // no exit code, and in an NTSTATUS code on Windows; a panic ends in 101.
    let output = bin().arg("check").arg(dir.path()).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "the check did not finish - status: {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
