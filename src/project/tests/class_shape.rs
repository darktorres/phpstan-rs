//! `Project::check_class_shapes`: closed-by-default class shapes and
//! `[[parent_calls]]`.

use std::path::PathBuf;

use crate::parent_calls::{HasContent, Invocation, ParentCallRule, default_rules};

use super::super::*;

fn check(source: &str, closed: bool, rules: &[ParentCallRule]) -> Vec<String> {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("a.php"), source)
        .unwrap();
    project.finish_loading();
    let mut out: Vec<String> = project
        .check_class_shapes(closed, rules)
        .into_values()
        .flatten()
        .map(|d| {
            let fix = d
                .fix
                .map(|f| format!(" [fix: {:?}]", f.replacement))
                .unwrap_or_default();
            format!("{} {}: {}{fix}", d.span.start.line, d.identifier, d.message)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn closed_class_shapes_skip_what_the_project_extends_or_overrides() {
    let source = "<?php
class Open {}
class Extended {}
class Child extends Extended {}
final class Sealed {
    protected int $count = 0;
    protected function helper(): void {}
    protected const LIMIT = 1;
}
abstract class Base {
    public function run(): void {}
    public function hook(): void {}
    abstract public function step(): void;
    final public function done(): void {}
    private function internal(): void {}
}
final class Impl extends Base {
    public function hook(): void {}
    public function step(): void {}
}
/** @ORM\\Entity */
class Mapped {}
";
    assert_eq!(
        check(source, true, &[]),
        vec![
            "11 method.notFinalInAbstractClass: Method Base::run() is not final, but since the containing class is abstract, it should be.",
            "2 class.notFinal: Class Open is not final.",
            "4 class.notFinal: Class Child is not final.",
            "6 property.protectedInFinalClass: Property Sealed::$count is protected, but since the containing class is final, it can be private.",
            "7 method.protectedInFinalClass: Method Sealed::helper() is protected, but since the containing class is final, it can be private.",
            "8 classConstant.protectedInFinalClass: Constant Sealed::LIMIT is protected, but since the containing class is final, it can be private.",
        ]
    );
}

#[test]
fn a_protected_member_the_parent_declares_stays_protected() {
    let source = "<?php
abstract class P { protected function tpl(): void {} abstract protected function x(): void; }
final class C extends P { protected function tpl(): void {} protected function x(): void {} }
";
    let found = check(source, true, &[]);
    assert!(
        !found.iter().any(|f| f.contains("protectedInFinalClass")),
        "{found:?}"
    );
}

const TESTS: &str = "<?php
namespace PHPUnit\\Framework { abstract class TestCase {
    protected function setUp(): void {}
    protected function tearDown(): void {}
} }
namespace App\\Tests {
abstract class BaseTest extends \\PHPUnit\\Framework\\TestCase {
    protected function setUp(): void { $this->boot(); }
    protected function tearDown(): void { $this->boot(); }
    protected function boot(): void {}
}
final class MissingTest extends BaseTest {
    protected function setUp(): void {
        $this->x = 1;
    }
    protected function tearDown(): void {
        $this->x = 2;
    }
}
final class LateTest extends BaseTest {
    protected function setUp(): void {
        $this->x = 1;
        parent::setUp();
    }
}
final class GoodTest extends BaseTest {
    protected function setUp(): void {
        parent::setUp();
        $this->x = 1;
    }
}
final class DirectTest extends \\PHPUnit\\Framework\\TestCase {
    protected function setUp(): void { $this->x = 1; }
}
}
";

#[test]
fn test_hooks_must_call_a_parent_that_does_something() {
    assert_eq!(
        check(TESTS, false, &default_rules()),
        vec![
            "13 parentCall.missing: Method App\\Tests\\MissingTest::setUp() doesn't call parent::setUp(), which App\\Tests\\BaseTest::setUp() implements. [fix: \"parent::setUp();\\n        \"]",
            "16 parentCall.missing: Method App\\Tests\\MissingTest::tearDown() doesn't call parent::tearDown(), which App\\Tests\\BaseTest::tearDown() implements. [fix: \"\\n        parent::tearDown();\"]",
            "21 parentCall.notFirst: Method App\\Tests\\LateTest::setUp() should call parent::setUp() first.",
        ]
    );
}

#[test]
fn configured_rules_cover_never_and_content() {
    let source = "<?php
class P { public function boot(): void { echo 1; } public function init(): void { echo 2; } }
final class C extends P {
    public function boot(): void { parent::boot(); }
    public function init(): void { parent::init(); echo 3; }
}
";
    let rules = [
        ParentCallRule {
            class: None,
            method: "boot".to_string(),
            invocation: Invocation::Any,
            has_content: HasContent::Yes,
        },
        ParentCallRule {
            class: Some("P".to_string()),
            method: "init".to_string(),
            invocation: Invocation::Never,
            has_content: HasContent::Maybe,
        },
    ];
    assert_eq!(
        check(source, false, &rules),
        vec![
            "4 parentCall.noContent: Method C::boot() only calls parent::boot(); the override does nothing else.",
            "5 parentCall.forbidden: Method C::init() calls parent::init(), which it must not.",
        ]
    );
}
