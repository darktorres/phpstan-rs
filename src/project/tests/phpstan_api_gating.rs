//! Phase 40: `Project::set_phpstan_api_check_enabled` actually wires
//! `checker::phpstan_api::check_phpstan_api_usage` into `check_file`'s own
//! diagnostics when (and only when) turned on - the checker module's own
//! test suite (`checker::phpstan_api::tests`) covers the check's own
//! logic in isolation; this covers the `Project`-level gate around it.
//!
//! Deliberately uses a `PHPStan\`-namespaced class name
//! (`ZzzPhase40TestOnlyMarker`) that doesn't collide with any real class
//! `php_stubs::generated_stub_classes()` already merges into every
//! `Project::new()` unconditionally (Finding 4: the real `vendor/
//! phpstan-src/src` tree, bundled specifically so this project can
//! resolve `PHPStan\*` types even against a `phpstan/phpstan` install
//! that shipped no loose source, e.g. its `.phar` distribution) - a real
//! name like `PHPStan\Rules\Rule` would merge this test's own bare stand-
//! in declaration onto the *real*, richly-shaped interface already
//! loaded (`registry::ClassInfo::merge`'s own "stub source wins on
//! conflict" rule), pulling in unrelated diagnostics (e.g. real `Rule`'s
//! own abstract methods) that have nothing to do with what's under test
//! here.
use super::super::*;

fn load_two_file_project(gate_enabled: bool, marker_doc: &str) -> Project {
    let mut project = Project::new();
    project.set_phpstan_api_check_enabled(gate_enabled);
    project
        .load_file(
            PathBuf::from("vendor/phpstan/phpstan/src/ZzzPhase40TestOnlyMarker.php"),
            &format!(
                "<?php namespace PHPStan\\Rules; {marker_doc}class ZzzPhase40TestOnlyMarker {{}}"
            ),
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/MyRule.php"),
            "<?php namespace App; class MyRule extends \\PHPStan\\Rules\\ZzzPhase40TestOnlyMarker {}",
        )
        .unwrap();
    project.finish_loading();
    project
}

#[test]
fn the_gate_off_by_default_reports_nothing_from_this_checker() {
    let project = load_two_file_project(false, "");
    let d = project.check_file(&PathBuf::from("src/MyRule.php"));
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn enabling_the_gate_flags_the_non_api_extends() {
    let project = load_two_file_project(true, "");
    let d = project.check_file(&PathBuf::from("src/MyRule.php"));
    assert_eq!(d.len(), 1, "diagnostics: {d:?}");
    assert!(
        d[0].message
            .contains("extending PHPStan\\Rules\\ZzzPhase40TestOnlyMarker")
    );
    assert!(d[0].message.contains("missing @api"));
}

#[test]
fn enabling_the_gate_stays_quiet_once_the_base_class_is_tagged_api() {
    let project = load_two_file_project(true, "/** @api */\n        ");
    let d = project.check_file(&PathBuf::from("src/MyRule.php"));
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

/// Not a normal regression test (nothing in this project's own behavior
/// is asserted beyond "the class exists") - a documented, deliberate
/// sanity check that real PHPStan's own `PHPStan\Rules\Rule` interface,
/// as merged in unconditionally via `php_stubs::generated_stub_classes`,
/// really does carry the real `@api` tag once parsed through this
/// project's own docblock/registry pipeline - confirming Phase 40's own
/// central premise: no extra vendor-loading work is needed for this
/// checker to see accurate `@api` state for real PHPStan classes, since
/// Finding 4's stub pipeline already threads every real class's own doc
/// comment through `registry::ClassInfo::from_decl` exactly the same way
/// an ordinary project file's declaration would be.
#[test]
fn real_phpstan_rule_interface_is_recognized_as_api_via_the_generated_stub_pipeline() {
    let project = Project::new();
    let registry = project.registry();

    let rule = registry
        .classes
        .get("PHPStan\\Rules\\Rule")
        .expect("PHPStan\\Rules\\Rule should be present via the generated stub pipeline");
    assert!(rule.is_api, "expected PHPStan\\Rules\\Rule to be @api");

    // `PHPStan\Analyser\Scope` carries both tags in real PHPStan's own
    // source (confirmed directly against `vendor/phpstan-src/src/
    // Analyser/Scope.php`) - proves the `@api-do-not-implement` half of
    // the pipeline also survives all the way through, not just a bare
    // `@api`.
    let scope = registry
        .classes
        .get("PHPStan\\Analyser\\Scope")
        .expect("PHPStan\\Analyser\\Scope should be present via the generated stub pipeline");
    assert!(scope.is_api, "expected PHPStan\\Analyser\\Scope to be @api");
    assert!(
        scope.is_api_do_not_implement,
        "expected PHPStan\\Analyser\\Scope to be @api-do-not-implement"
    );

    // A real, ordinary internal class with no `@api` tag at all must not
    // come out tagged by accident (a `false`-by-default sanity check, not
    // just "the true case works").
    let resolver = registry
        .classes
        .get("PHPStan\\Analyser\\NodeScopeResolver")
        .expect("PHPStan\\Analyser\\NodeScopeResolver should be present via the generated stub pipeline");
    assert!(
        !resolver.is_api,
        "expected PHPStan\\Analyser\\NodeScopeResolver to NOT be @api"
    );
}
