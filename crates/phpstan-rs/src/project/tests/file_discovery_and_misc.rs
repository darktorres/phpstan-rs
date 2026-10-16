//! File discovery on disk, `Project` defaults, and other checks that don't
//! fit the incremental-update or Pest directory-scoping categories.

use super::super::*;
use super::helpers::*;
use crate::project::session::load_test_file;

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
    // check_all is a thin fold over the already-well-tested check_file,
    // and is what --check-project actually calls.
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("good.php"),
        "<?php function f(): int { return 1; }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("bad.php"),
        "<?php function g() { return $undefinedVar; }",
        false,
    );
    project.finish_loading();

    let all = project.check_all(&crate::project::worker_pool());
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
fn check_paths_restricts_output_but_keeps_whole_project_facts() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("good.php"),
        "<?php function f(): int { return 1; }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("bad.php"),
        "<?php function g() { return $undefinedVar; }",
        false,
    );
    project.finish_loading();

    let result = project.check_paths(&[PathBuf::from("bad.php")], &crate::project::worker_pool());

    assert_eq!(result.len(), 1, "got: {result:?}");
    assert!(
        result[&PathBuf::from("bad.php")]
            .iter()
            .any(|d| d.message.contains("undefined variable"))
    );
}

#[test]
fn check_paths_isolating_panics_behaves_like_check_paths_when_nothing_panics() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("good.php"),
        "<?php function f(): int { return 1; }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("bad.php"),
        "<?php function g() { return $undefinedVar; }",
        false,
    );
    project.finish_loading();
    let paths = vec![PathBuf::from("good.php"), PathBuf::from("bad.php")];

    let expected = project.check_paths(&paths, &crate::project::worker_pool());
    let (actual, panics) =
        project.check_paths_isolating_panics(&paths, &crate::project::worker_pool());

    assert!(panics.is_empty(), "got: {panics:?}");
    assert_eq!(actual.len(), expected.len());
    for (path, diagnostics) in &expected {
        assert_eq!(
            actual[path].iter().map(|d| &d.message).collect::<Vec<_>>(),
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>(),
            "mismatch for {path:?}"
        );
    }
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
    load_test_file(
        &mut project,
        PathBuf::from("user.php"),
        "<?php namespace App\\Models; class User { public function getName(): string { return 'x'; } }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("greeter.php"),
        "<?php namespace App\\Services;\nuse App\\Models\\User;\nfunction greet(User $u): string { return $u->getName(); }",
        false,
    );
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
    load_test_file(
        &mut bad_project,
        PathBuf::from("user.php"),
        "<?php namespace App\\Models; class User { public function getName(): string { return 'x'; } }",
        false,
    );
    load_test_file(
        &mut bad_project,
        PathBuf::from("greeter.php"),
        "<?php namespace App\\Services;\nuse App\\Models\\User;\nfunction greet(User $u): string { return $u->totallyMadeUp(); }",
        false,
    );
    bad_project.finish_loading();
    let bad_diagnostics = bad_project.check_file(&PathBuf::from("greeter.php"));
    assert!(
        bad_diagnostics
            .iter()
            .any(|d| d.message.contains("totallyMadeUp")),
        "expected an undefined-method diagnostic, got: {bad_diagnostics:?}"
    );
}

#[test]
fn deferred_registry_passes_all_run_through_the_real_project_path() {
    // `Project::finish_loading`/`update_file` (what `check-project` and
    // the LSP actually use) call the individual registry passes
    // directly rather than through `Registry::index_and_flatten` - they
    // must still include `resolve_method_tags`/
    // `resolve_deferred_type_imports`/`resolve_deferred_mixins`, or
    // @method/@mixin/cross-class @phpstan-import-type silently stop
    // working through the real CLI. One project, exercising all three
    // at once, through `check_all` - exactly what `--check-project`
    // itself calls.
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("source.php"),
        "<?php /** @phpstan-type ValueType string */ class Types {} class Collection { public function count(): int { return 0; } }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("consumer.php"),
        concat!(
            "<?php\n",
            "/**\n",
            " * @phpstan-import-type ValueType from Types as ImportedValueType\n",
            " * @mixin Collection\n",
            " * @method int getX()\n",
            " */\n",
            "class Consumer {\n",
            "    function needsInt(int $x): void {}\n",
            "    /** @return ImportedValueType */\n",
            "    function readValue() { return 'x'; }\n",
            "    function test(): void {\n",
            "        $this->needsInt($this->readValue());\n",
            "        $this->needsInt($this->count());\n",
            "        $this->needsInt($this->getX());\n",
            "    }\n",
            "}\n",
        ),
        false,
    );
    project.finish_loading();

    let all = project.check_all(&crate::project::worker_pool());
    let consumer_diagnostics = &all[&PathBuf::from("consumer.php")];
    // Only the @phpstan-import-type call is a real type mismatch (string
    // isn't int) - @mixin's count() and @method's getX() both return int
    // already, so those two calls should be silent.
    assert_eq!(
        consumer_diagnostics.len(),
        1,
        "got: {consumer_diagnostics:?}"
    );
    assert_eq!(consumer_diagnostics[0].identifier, "argument.type");
    assert!(
        consumer_diagnostics[0].message.contains("string given"),
        "got: {consumer_diagnostics:?}"
    );
}

#[test]
fn analyzed_paths_and_is_analyzed_reflect_how_each_file_was_loaded() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("vendor/lib.php"),
        "<?php function libHelper(): int { return 1; }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("src/app.php"),
        "<?php function appHelper(): int { return 1; }",
        true,
    );
    project.finish_loading();

    assert!(!project.is_analyzed(&PathBuf::from("vendor/lib.php")));
    assert!(project.is_analyzed(&PathBuf::from("src/app.php")));
    assert!(!project.is_analyzed(&PathBuf::from("never/loaded.php")));

    let analyzed = project.analyzed_paths();
    assert_eq!(analyzed, vec![PathBuf::from("src/app.php")]);
}

#[test]
fn update_file_defaults_a_never_before_seen_path_by_its_excluded_dir_status() {
    let mut project = Project::new();

    project
        .update_file(&PathBuf::from("vendor/acme/Foo.php"), "<?php class Foo {}")
        .unwrap();
    assert!(
        !project.is_analyzed(&PathBuf::from("vendor/acme/Foo.php")),
        "a never-before-seen path under vendor/ must default to context, not analyzed"
    );

    project
        .update_file(&PathBuf::from("src/Foo.php"), "<?php class Bar {}")
        .unwrap();
    assert!(
        project.is_analyzed(&PathBuf::from("src/Foo.php")),
        "a never-before-seen path outside any excluded dir must default to analyzed"
    );
}
