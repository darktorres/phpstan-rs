//! File discovery on disk, `Project` defaults, and other checks that don't
//! fit the incremental-update or Pest directory-scoping categories.

use super::super::*;
use super::helpers::*;

#[test]
fn collect_php_files_recurses_into_subdirectories_and_skips_non_php() {
    let dir = TempDir::new("collect");
    dir.write("a.php", "<?php");
    dir.write("nested/b.php", "<?php");
    dir.write("notes.txt", "not php");

    let mut files = Vec::new();
    collect_php_files(&dir.0, &mut files);
    files.sort();

    assert_eq!(files, vec![dir.0.join("a.php"), dir.0.join("nested/b.php")]);
}

#[test]
fn default_is_equivalent_to_new() {
    let project = Project::default();
    assert_eq!(project.file_count(), 0);
}

#[test]
fn check_all_covers_every_loaded_file_not_just_one() {
    // check_all is a thin fold over the already-well-tested check_file
    // (and is what --check-project actually calls), but had zero
    // direct test executions per cargo llvm-cov - cheap to close.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("good.php"),
            "<?php function f(): int { return 1; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("bad.php"),
            "<?php function g() { return $undefinedVar; }",
        )
        .unwrap();
    project.finish_loading();

    let all = project.check_all();
    assert_eq!(project.file_count(), 2);
    assert_eq!(all.len(), 2);
    assert!(all[&PathBuf::from("good.php")].is_empty());
    assert!(
        all[&PathBuf::from("bad.php")]
            .iter()
            .any(|d| d.message.contains("undefined variable"))
    );
}

#[test]
fn cross_file_use_import_and_method_calls_resolve_correctly() {
    // Every other cross-file test in this module uses bare,
    // non-namespaced classes with no `use` import at all - the exact
    // combination (separate namespaces + a `use` import + a method
    // call checked against the imported class, all across a real
    // multi-file Project rather than a single Checker) that a
    // regression in NameContext resolution, FQN-keyed lookups, or
    // undefined-method detection could plausibly break without any
    // existing test noticing.
    let mut project = Project::new();
    project.load_file(
        PathBuf::from("user.php"),
        "<?php namespace App\\Models; class User { public function getName(): string { return 'x'; } }",
    ).unwrap();
    project.load_file(
        PathBuf::from("greeter.php"),
        "<?php namespace App\\Services;\nuse App\\Models\\User;\nfunction greet(User $u): string { return $u->getName(); }",
    ).unwrap();
    project.finish_loading();

    let diagnostics = project.check_file(&PathBuf::from("greeter.php"));
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics for a real cross-namespace method call: {diagnostics:?}"
    );

    // Same setup, but calling a method User genuinely doesn't have -
    // proves undefined-method detection also works through this same
    // cross-file, use-imported path, not just same-file/same-namespace.
    let mut bad_project = Project::new();
    bad_project.load_file(
        PathBuf::from("user.php"),
        "<?php namespace App\\Models; class User { public function getName(): string { return 'x'; } }",
    ).unwrap();
    bad_project.load_file(
        PathBuf::from("greeter.php"),
        "<?php namespace App\\Services;\nuse App\\Models\\User;\nfunction greet(User $u): string { return $u->totallyMadeUp(); }",
    ).unwrap();
    bad_project.finish_loading();
    let bad_diagnostics = bad_project.check_file(&PathBuf::from("greeter.php"));
    assert!(
        bad_diagnostics
            .iter()
            .any(|d| d.message.contains("totallyMadeUp")),
        "expected an undefined-method diagnostic, got: {bad_diagnostics:?}"
    );
}
