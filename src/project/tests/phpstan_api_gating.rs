//! `Project::set_phpstan_api_check_enabled` actually wires
//! `checker::phpstan_api::check_phpstan_api_usage` into `check_file`'s own
//! diagnostics when (and only when) turned on - the checker module's own
//! test suite (`checker::phpstan_api::tests`) covers the check's own
//! logic in isolation; this covers the `Project`-level gate around it.
//!
//! Deliberately uses a `PHPStan\`-namespaced class name
//! (`ZzzPhase40TestOnlyMarker`) that doesn't collide with any real class
//! `php_stubs::generated_stub_classes()` already merges into every
//! `Project::new()` unconditionally (the vendored `vendor/phpstan-src/src`
//! tree, bundled specifically so this project can resolve `PHPStan\*`
//! types even against a `phpstan/phpstan` install that shipped no loose
//! source, e.g. its `.phar` distribution) - a real name like
//! `PHPStan\Rules\Rule` would merge this test's own bare stand-in
//! declaration onto the real, richly-shaped interface already loaded
//! (`registry::ClassInfo::merge`'s "stub source wins on conflict" rule),
//! pulling in unrelated diagnostics (e.g. real `Rule`'s own abstract
//! methods) that have nothing to do with what's under test here.
use super::super::*;
use super::helpers::without_missing_typehint_diagnostics;

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

// The vendor-directory half of `is_phpstan_code` - only reachable
// through a real `Project`/`PathBuf` harness, since the checker-level
// `Registry::build`-only test suite never has real paths to populate
// `Registry::phpstan_class_files` from at all.

#[test]
fn a_phpstan_namespaced_caller_referencing_a_vendor_loaded_phpstan_class_is_flagged() {
    // Mirrors real PHPStan's own dogfooding shape: a PHPStan extension
    // package (e.g. phpstan-doctrine) is itself namespaced `PHPStan\*`
    // and calls into real, vendor-loaded PHPStan internals.
    //
    // Deliberately uses a synthetic `ZzzPhase40TrinaryLogicMarker` name,
    // not the real `PHPStan\TrinaryLogic` - that real class is already
    // bundled (with a real `@api` tag) via `php_stubs::generated_stub_
    // classes()`'s own unconditional `vendor/phpstan-src/src` merge into
    // every `Project::new()`, so this test's own bare, `@api`-less
    // re-declaration of it would *merge* onto the real one rather than
    // stand alone (`registry::index::insert_built_decl`'s own documented
    // "two unconditional declarations of the same name merge, permissive
    // either-side-wins" convention for a non-analyzed/context file) -
    // silently inheriting the real class's own `@api` status and making
    // this test's own diagnostic never fire, for a reason unrelated to
    // what it means to test. Same collision-avoidance reasoning this
    // file's own third test already documents for `ZzzPhase40BaseMarker`.
    let mut project = Project::new();
    project.set_phpstan_api_check_enabled(true);
    project
        .load_file(
            PathBuf::from("vendor/phpstan/phpstan/src/ZzzPhase40TrinaryLogicMarker.php"),
            "<?php namespace PHPStan; class ZzzPhase40TrinaryLogicMarker { public static function check() {} }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/Doctrine/MyRule.php"),
            "<?php namespace PHPStan\\Doctrine; class MyRule { public function run() { \\PHPStan\\ZzzPhase40TrinaryLogicMarker::check(); } }",
        )
        .unwrap();
    project.finish_loading();
    let d = without_missing_typehint_diagnostics(
        project.check_file(&PathBuf::from("src/Doctrine/MyRule.php")),
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("calling PHPStan\\ZzzPhase40TrinaryLogicMarker::check()")),
        "diagnostics: {d:?}"
    );
}

#[test]
fn a_phpstan_namespaced_caller_referencing_a_sibling_phpstan_namespaced_class_is_not_flagged() {
    // Same calling shape as above, but the target is a sibling of the
    // caller's own package (not vendor-loaded) - real PHPStan's own
    // `isPhpStanCode` correctly stays silent here (a PHPStan extension
    // package calling its own internal helper).
    let mut project = Project::new();
    project.set_phpstan_api_check_enabled(true);
    project
        .load_file(
            PathBuf::from("src/Doctrine/Bar.php"),
            "<?php namespace PHPStan\\Doctrine; class Bar { public static function check() {} }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/Doctrine/MyRule.php"),
            "<?php namespace PHPStan\\Doctrine; class MyRule { public function run() { Bar::check(); } }",
        )
        .unwrap();
    project.finish_loading();
    let d = without_missing_typehint_diagnostics(
        project.check_file(&PathBuf::from("src/Doctrine/MyRule.php")),
    );
    // This fixture's own `namespace PHPStan\Doctrine;` declaration is
    // itself real, separate signal (`phpstanApi.phpstanNamespace`,
    // unrelated to what this test means to exercise) - only the static-
    // call-specific diagnostic is what "not flagged" refers to here.
    assert!(
        !d.iter().any(|x| x.identifier == "phpstanApi.method"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_static_call_to_an_inherited_phpstan_method_through_a_non_phpstan_subclass_is_flagged() {
    // Mirrors PHPStan's `ApiStaticCallRule`: it gates on the resolved
    // method's declaring class, never the literally-written class
    // name's own namespace - `MyNonPhpStanSubclass::inheritedMethod()`
    // must still be flagged, even though the written class name
    // (`MyNonPhpStanSubclass`) is itself `App`-namespaced.
    let mut project = Project::new();
    project.set_phpstan_api_check_enabled(true);
    project
        .load_file(
            PathBuf::from("vendor/phpstan/phpstan/src/ZzzPhase40BaseMarker.php"),
            "<?php namespace PHPStan\\Rules; class ZzzPhase40BaseMarker { public static function inheritedMethod() {} }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/MyNonPhpStanSubclass.php"),
            "<?php namespace App; class MyNonPhpStanSubclass extends \\PHPStan\\Rules\\ZzzPhase40BaseMarker {}",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/Caller.php"),
            "<?php namespace App; class Caller { public function run() { MyNonPhpStanSubclass::inheritedMethod(); } }",
        )
        .unwrap();
    project.finish_loading();
    let d =
        without_missing_typehint_diagnostics(project.check_file(&PathBuf::from("src/Caller.php")));
    assert!(
        d.iter().any(|x| x
            .message
            .contains("calling PHPStan\\Rules\\ZzzPhase40BaseMarker::inheritedMethod()")),
        "diagnostics: {d:?}"
    );
}

#[test]
fn a_bare_namespace_phpstan_declaration_with_no_trailing_backslash_is_recognized() {
    // `is_phpstan_namespace`'s bare-`PHPStan` (no trailing backslash)
    // case.
    let mut project = Project::new();
    project.set_phpstan_api_check_enabled(true);
    project
        .load_file(
            PathBuf::from("vendor/phpstan/phpstan/src/ZzzPhase40BareNsMarker.php"),
            "<?php namespace PHPStan; class ZzzPhase40BareNsMarker {}",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/MyRule.php"),
            "<?php namespace App; class MyRule extends \\PHPStan\\ZzzPhase40BareNsMarker {}",
        )
        .unwrap();
    project.finish_loading();
    let d = project.check_file(&PathBuf::from("src/MyRule.php"));
    assert_eq!(d.len(), 1, "diagnostics: {d:?}");
}

/// Not a normal regression test (nothing in this project's own behavior
/// is asserted beyond "the class exists") - a documented, deliberate
/// sanity check that PHPStan's `PHPStan\Rules\Rule` interface, as merged
/// in unconditionally via `php_stubs::generated_stub_classes`, really
/// does carry the `@api` tag once parsed through this project's own
/// docblock/registry pipeline: no extra vendor-loading work is needed
/// for this checker to see accurate `@api` state for PHPStan classes,
/// since the stub pipeline already threads every real class's doc
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
