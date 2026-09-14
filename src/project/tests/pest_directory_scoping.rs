//! Pest's `uses(...)->in(...)` directory-scoped bindings, and the
//! `beforeEach`/`afterEach` chain that attaches dynamic properties to `$this`.

use super::super::*;
use super::helpers::*;

#[test]
fn pest_directory_scoped_uses_resolves_this_to_the_bound_class() {
    // Real shape: Piwigo's own tests/Pest.php - `uses(X::class)->in
    // ('Browser/Api')` binds only that subtree; a file outside it
    // falls through to the default. Calling a real method on the
    // bound class (not just tolerated via Mixed) proves `$this`
    // actually resolved to the directory-scoped class, not the
    // default fallback.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("tests/Pest.php"),
            "<?php uses(ApiAssertions::class)->in('Browser/Api');",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Support/ApiAssertions.php"),
            "<?php class ApiAssertions { public function assertContract(): void {} }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Browser/Api/FooTest.php"),
            "<?php function it($description, $callback) {} it('works', function (): void { $this->assertContract(); });",
        )
        .unwrap();
    project.finish_loading();

    let d = without_missing_typehint_diagnostics(
        project.check_file(&PathBuf::from("tests/Browser/Api/FooTest.php")),
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_directory_scoped_uses_still_flags_a_real_undefined_method() {
    // Same setup as above, but calling a method the bound class
    // doesn't declare - proves the directory-scoped binding resolved
    // a real, specific class (not Mixed, which would tolerate this).
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("tests/Pest.php"),
            "<?php uses(ApiAssertions::class)->in('Browser/Api');",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Support/ApiAssertions.php"),
            "<?php class ApiAssertions { public function assertContract(): void {} }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Browser/Api/FooTest.php"),
            "<?php function it($description, $callback) {} it('works', function (): void { $this->totallyUndefinedMethod(); });",
        )
        .unwrap();
    project.finish_loading();

    let d = project.check_file(&PathBuf::from("tests/Browser/Api/FooTest.php"));
    assert!(
        d.iter().any(|x| x.message.contains("undefined method")),
        "expected an undefined-method diagnostic, got: {d:?}"
    );
}

#[test]
fn pest_file_outside_the_bound_directory_does_not_get_the_directory_scoped_type() {
    // A file outside `Browser/Api` must not see ApiAssertions's
    // methods - falls through to the default (Mixed, since
    // PHPUnit\Framework\TestCase isn't registered in this bare
    // project), which tolerates the call - a weaker, but still
    // correct, assertion than the bound-directory tests above (this
    // project has no way to positively assert "resolved to Mixed"
    // other than "didn't inherit the directory binding's own type").
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("tests/Pest.php"),
            "<?php uses(ApiAssertions::class)->in('Browser/Api');",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Support/ApiAssertions.php"),
            "<?php class ApiAssertions { public function assertContract(): void {} }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Unit/FooTest.php"),
            "<?php function it($description, $callback) {} it('works', function (): void { $this->assertContract(); });",
        )
        .unwrap();
    project.finish_loading();

    let d = without_missing_typehint_diagnostics(
        project.check_file(&PathBuf::from("tests/Unit/FooTest.php")),
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_file_local_before_each_property_is_readable_from_another_closure_in_the_same_file() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("tests/FooTest.php"),
            "<?php
            function beforeEach($callback) {}
            function it($description, $callback) {}
            beforeEach(function (): void { $this->root = 'x'; });
            it('works', function (): void { echo $this->root; });",
        )
        .unwrap();
    project.finish_loading();

    let d = without_missing_typehint_diagnostics(
        project.check_file(&PathBuf::from("tests/FooTest.php")),
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_directory_scoped_before_each_property_is_readable_across_files() {
    // Real shape: Piwigo's own tests/Pest.php - `uses()->beforeEach
    // (...)->afterEach(...)->in('Unit')` chains an intermediate
    // afterEach() between uses() and the terminal in() - proves
    // walk_uses_chain doesn't require beforeEach to sit immediately
    // next to uses().
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("tests/Pest.php"),
            "<?php
            function afterEach($callback) {}
            uses()
                ->beforeEach(function (): void { $this->root = 'x'; })
                ->afterEach(function (): void {})
                ->in('Unit');",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Unit/FooTest.php"),
            "<?php
            function it($description, $callback) {}
            it('works', function (): void { echo $this->root; });",
        )
        .unwrap();
    project.finish_loading();

    let d = without_missing_typehint_diagnostics(
        project.check_file(&PathBuf::from("tests/Unit/FooTest.php")),
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_directory_scoped_before_each_property_does_not_leak_outside_its_directory() {
    // Binds $this to a real, registered class in both directories (via
    // an unrelated `uses(RealTestBase::class)` for Browser/, matching
    // `pest_file_outside_the_bound_directory_...`'s own established
    // pattern) - without a concrete class, `$this` resolves to Mixed
    // in this bare test harness (no real PHPUnit\Framework\TestCase
    // registered) and undefined-property checking never fires at all.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("tests/Pest.php"),
            "<?php
            uses(RealTestBase::class)->in('Browser');
            uses()
                ->beforeEach(function (): void { $this->root = 'x'; })
                ->in('Unit');",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Support/RealTestBase.php"),
            "<?php class RealTestBase {}",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("tests/Browser/FooTest.php"),
            "<?php
            function it($description, $callback) {}
            it('works', function (): void { echo $this->root; });",
        )
        .unwrap();
    project.finish_loading();

    let d = project.check_file(&PathBuf::from("tests/Browser/FooTest.php"));
    assert!(
        d.iter().any(|x| x.message.contains("$root")),
        "expected an undefined-property diagnostic, got: {d:?}"
    );
}
