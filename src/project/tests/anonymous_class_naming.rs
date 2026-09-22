//! An anonymous class (`new class { ... }`) instance registered into the
//! registry under its own stable synthetic name
//! (`registry::collect_anonymous_classes`), instead of inferred as bare
//! `Type::Mixed(false)` - the exact `tests/Unit/Http/Middleware/
//! ApiIdempotencyMiddlewareTest.php:51`/`SentryMiddlewareTest.php:119`
//! corpus shape (an anonymous-class instance placed into a typed array
//! shape/argument whose declared type names an interface it implements).

use super::super::*;
use super::helpers::*;

#[test]
fn anonymous_class_return_type_satisfies_its_own_implemented_interface() {
    // Mirrors `ApiIdempotencyMiddlewareTest.php:51` exactly: before this
    // fix, the anonymous class instance's own inferred type was bare
    // `Mixed`, which doesn't satisfy a declared `array{0: Handler, ...}`
    // return shape's own `Handler`-typed slot - a real, confirmed corpus
    // false positive (`return.type`).
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("src/Handler.php"),
            "<?php interface Handler { public function handle(): int; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/build.php"),
            "<?php
            /** @return array{0: Handler, 1: callable(): int} */
            function build(): array {
                $handler = new class implements Handler {
                    public function handle(): int { return 1; }
                };
                return [$handler, function (): int { return 2; }];
            }",
        )
        .unwrap();
    project.finish_loading();

    let d =
        without_missing_typehint_diagnostics(project.check_file(&PathBuf::from("src/build.php")));
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn anonymous_class_not_implementing_the_required_interface_still_flags_return_type_mismatch() {
    // Negative control for the test above: proves the fix is real type
    // resolution, not blanket leniency toward every anonymous class
    // instance regardless of shape.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("src/Handler.php"),
            "<?php interface Handler { public function handle(): int; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/build.php"),
            "<?php
            /** @return array{0: Handler, 1: callable(): int} */
            function build(): array {
                $notAHandler = new class { public function other(): int { return 1; } };
                return [$notAHandler, function (): int { return 2; }];
            }",
        )
        .unwrap();
    project.finish_loading();

    let d =
        without_missing_typehint_diagnostics(project.check_file(&PathBuf::from("src/build.php")));
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "expected a return.type diagnostic, got: {d:?}"
    );
}

#[test]
fn an_anonymous_class_instances_own_extra_method_is_resolved_alongside_its_interface() {
    // The exact shape the already-reverted "ancestor-aliasing shortcut"
    // (see ROADMAP.md's own Reverted section) got wrong: a "spy" that
    // implements a real interface but also declares its own extra member
    // a caller reads directly. Registering the anonymous class's own full
    // declaration (not just aliasing to the interface it implements) means
    // both a real interface method call AND the class's own extra method
    // call resolve, while a genuinely undefined method is still flagged.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("src/Handler.php"),
            "<?php interface Handler { public function handle(): int; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("src/build.php"),
            "<?php
            function build(): int {
                $spy = new class implements Handler {
                    public function handle(): int { return 1; }
                    public function callCount(): int { return 0; }
                };
                return $spy->handle() + $spy->callCount();
            }",
        )
        .unwrap();
    project.finish_loading();

    let d =
        without_missing_typehint_diagnostics(project.check_file(&PathBuf::from("src/build.php")));
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_a_genuinely_undefined_method_on_an_anonymous_class_instance_is_still_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("src/build.php"),
            "<?php
            function build(): int {
                $obj = new class { public function handle(): int { return 1; } };
                return $obj->doesNotExist();
            }",
        )
        .unwrap();
    project.finish_loading();

    let d =
        without_missing_typehint_diagnostics(project.check_file(&PathBuf::from("src/build.php")));
    assert!(
        d.iter().any(|x| x.identifier == "method.notFound"),
        "expected a method.notFound diagnostic, got: {d:?}"
    );
}
