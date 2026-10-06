//! PHPStan's own rule interface, loaded through the embedded stub
//! pipeline, counts as `@api`. The gate itself is covered by the fixtures
//! under `tests/fixtures/project/phpstan_api_gating`.
use std::path::Path;

use super::super::test_support::Files;

/// Not a normal regression test (nothing in this project's own behavior
/// is asserted beyond "the class exists") - a documented, deliberate
/// sanity check that PHPStan's `PHPStan\Rules\Rule` interface, as merged
/// in once a loaded file names `PHPStan\` (`Project::sync_phpstan_stubs`), really
/// does carry the `@api` tag once parsed through this project's own
/// docblock/registry pipeline: no extra vendor-loading work is needed
/// for this checker to see accurate `@api` state for PHPStan classes,
/// since the stub pipeline already threads every real class's doc
/// comment through `registry::ClassReflection::from_decl` exactly the same way
/// an ordinary project file's declaration would be.
#[test]
fn real_phpstan_rule_interface_is_recognized_as_api_via_the_generated_stub_pipeline() {
    let project = Files::new()
        .analysed(
            "rule.php",
            "<?php function f(\\PHPStan\\Rules\\Rule $rule): void {}",
        )
        .load();
    let registry = project.registry();

    let rule = registry
        .classes()
        .get("PHPStan\\Rules\\Rule")
        .expect("PHPStan\\Rules\\Rule should be present via the generated stub pipeline");
    assert!(rule.is_api(), "expected PHPStan\\Rules\\Rule to be @api");

    // `PHPStan\Analyser\Scope` carries both tags in PHPStan's own source -
    // proves the `@api-do-not-implement` half of the pipeline also
    // survives all the way through, not just a bare `@api`.
    let scope = registry
        .classes()
        .get("PHPStan\\Analyser\\Scope")
        .expect("PHPStan\\Analyser\\Scope should be present via the generated stub pipeline");
    assert!(
        scope.is_api(),
        "expected PHPStan\\Analyser\\Scope to be @api"
    );
    assert!(
        scope.is_api_do_not_implement(),
        "expected PHPStan\\Analyser\\Scope to be @api-do-not-implement"
    );

    // A real, ordinary internal class with no `@api` tag at all must not
    // come out tagged by accident (a `false`-by-default sanity check, not
    // just "the true case works").
    let resolver = registry.classes()
        .get("PHPStan\\Analyser\\NodeScopeResolver")
        .expect("PHPStan\\Analyser\\NodeScopeResolver should be present via the generated stub pipeline");
    assert!(
        !resolver.is_api(),
        "expected PHPStan\\Analyser\\NodeScopeResolver to NOT be @api"
    );
}

#[test]
fn finishing_an_edit_that_introduces_phpstan_resolves_stub_functions_by_id() {
    let path = Path::new("file.php");
    let mut project = Files::new()
        .analysed("file.php", "<?php function f(): void {}")
        .load();
    project.check_file(path);

    project
        .update_file(path, r"<?php \PHPStan\Testing\assertType('int', 1);")
        .unwrap();

    assert!(
        project
            .registry()
            .lookup_function("PHPStan\\Testing\\assertType")
            .is_some(),
        "the dynamically merged PHPStan stub must have a declaration id"
    );
}
