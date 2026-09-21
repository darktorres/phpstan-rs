//! `checker::whole_program::dead_members` (PHASE_BACKLOG Phase 86's
//! B1), wired through `Project::check_whole_program` - opt-in, off by
//! default (see `registry::Registry::check_dead_members`'s own doc
//! comment). Every fixture loads via `load_analyzed_file`, not
//! `load_file` - see `dead_enum_cases`'s own module doc comment for why.

use super::super::*;

fn project_with_check_enabled() -> Project {
    let mut project = Project::new();
    project.set_check_dead_members(true);
    project
}

#[test]
fn off_by_default() {
    let mut project = Project::new();
    project
        .load_analyzed_file(
            PathBuf::from("a.php"),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_never_called_public_method_is_flagged_when_enabled() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).expect("expected findings for a.php");
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "method.unused" && d.message.contains("Foo::bar")),
        "got: {diags:?}"
    );
}

#[test]
fn a_method_called_from_a_different_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f(Foo $x) { $x->bar(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("bar")),
        "got: {diags:?}"
    );
}

#[test]
fn a_never_read_protected_property_is_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Foo { protected $x; }")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "property.unused" && d.message.contains("Foo::x")),
        "got: {diags:?}"
    );
}

#[test]
fn a_property_read_from_a_subclass_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Foo { protected $x; }")
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php class Bar extends Foo { function f() { return $this->x; } }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Foo::x")),
        "got: {diags:?}"
    );
}

#[test]
fn a_never_used_public_const_is_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Foo { const BAR = 1; }")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "classConstant.unused" && d.message.contains("Foo::BAR")),
        "got: {diags:?}"
    );
}

#[test]
fn a_constructor_is_never_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function __construct() {} }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("__construct")),
        "got: {diags:?}"
    );
}

#[test]
fn a_phpunit_test_method_and_lifecycle_callback_are_never_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            r#"<?php
                namespace PHPUnit\Framework {
                    class TestCase {}
                }
                namespace {
                    class MyTest extends \PHPUnit\Framework\TestCase {
                        public function setUp(): void {}
                        public function testSomething(): void {}
                    }
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("setUp") || d.message.contains("testSomething")),
        "got: {diags:?}"
    );
}

#[test]
fn a_tostring_method_is_never_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function __toString(): string { return ''; } }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("__toString")),
        "got: {diags:?}"
    );
}

#[test]
fn a_countable_count_method_is_never_flagged_even_via_a_parent_class() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php abstract class Base implements Countable {}\n\
             class Foo extends Base { public function count(): int { return 0; } }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Foo::count")),
        "got: {diags:?}"
    );
}

#[test]
fn overriding_an_arbitrary_non_spl_interfaces_method_is_never_flagged() {
    // Proves the exclusion is a *generic* override check, not a table
    // of hardcoded SPL interface names - a made-up interface's own
    // contract method is excluded exactly the same way.
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php interface NodeVisitor { public function enterNode(): void; }\n\
             class MyVisitor implements NodeVisitor { public function enterNode(): void {} }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("enterNode")),
        "got: {diags:?}"
    );
}

#[test]
fn a_literal_dynamic_property_access_counts_as_real_usage() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Foo { public $x; }")
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f(Foo $obj) { return $obj->{'x'}; }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Foo::x")),
        "got: {diags:?}"
    );
}

#[test]
fn a_literal_dynamic_method_call_counts_as_real_usage() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f(Foo $obj) { $obj->{'bar'}(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Foo::bar")),
        "got: {diags:?}"
    );
}

#[test]
fn a_genuinely_dynamic_method_call_on_a_known_type_escapes_the_whole_class() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function bar(): void {} public function baz(): void {} }",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f(Foo $obj, string $name) { $obj->{$name}(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Foo::bar")),
        "got: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("Foo::baz")),
        "got: {diags:?}"
    );
}

#[test]
fn a_dynamic_call_escapes_the_declaring_ancestor_reached_through_a_subclass_instance() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Base { protected function bar(): void {} }\n\
             class Sub extends Base {}",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f(Sub $obj, string $name) { $obj->{$name}(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Base::bar")),
        "got: {diags:?}"
    );
}

#[test]
fn a_dynamic_call_on_an_interface_typed_receiver_escapes_every_real_implementor() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php interface Greeter {}\n\
             class Foo implements Greeter { public function greet(): void {} }",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f(Greeter $obj, string $name) { $obj->{$name}(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Foo::greet")),
        "got: {diags:?}"
    );
}

#[test]
fn a_dynamic_call_on_a_mixed_typed_receiver_gives_no_escape_protection() {
    // Disclosed, not fixed - see `checker::whole_program::dead_members`'s
    // own module doc comment. A genuinely unresolvable receiver can't be
    // attributed to any class, so this still flags Foo::bar as dead -
    // proving the gap is real, not silently "handled" by accident.
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f($obj, string $name) { $obj->{$name}(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        diags.iter().any(|d| d.message.contains("Foo::bar")),
        "got: {diags:?}"
    );
}

#[test]
fn a_bare_variable_dynamic_call_gives_no_escape_protection_either() {
    // Disclosed, not fixed - `$obj->$name()` (unlike the braced
    // `$obj->{$name}()` this check's own escape hatch handles) parses
    // as an ordinary `MethodCall` named after the *variable's own
    // source name* ("name"), never its runtime value - see `checker::
    // whole_program::dead_members`'s own module doc comment. Foo::bar
    // still gets flagged dead - proving this pre-existing parser
    // modeling choice really does carry the disclosed risk, not
    // silently escaping it by accident.
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php function f(Foo $obj, string $name) { $obj->$name(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        diags.iter().any(|d| d.message.contains("Foo::bar")),
        "got: {diags:?}"
    );
}

#[test]
fn a_dead_member_in_a_non_analyzed_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("vendor/lib/a.php");
    project
        .load_file(
            path.clone(),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(
        !results.contains_key(&path),
        "a vendor-only file should never gain a new results entry, got: {results:?}"
    );
}
