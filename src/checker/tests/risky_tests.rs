//! Opt-in PHPUnit's `--fail-on-risky` equivalent (`test.risky`): a test
//! method/Pest test closure with zero assertion-shaped calls anywhere
//! in its body.

use super::helpers::*;

/// Wraps `class_body` (a real class declaration, or several) in the
/// bracketed-namespace form (real, parser-supported PHP syntax) so a
/// fake `PHPUnit\Framework\TestCase` can be declared in its own real
/// namespace without also swallowing the test class itself into it.
fn with_test_case(class_body: &str) -> String {
    format!(
        r#"<?php
        namespace PHPUnit\Framework {{
            class TestCase {{}}
        }}
        namespace {{
            {class_body}
        }}
        "#
    )
}

#[test]
fn off_by_default() {
    let d = diagnostics_for(&with_test_case(
        r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $x = 1 + 1;
                }
            }
        "#,
    ));
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn a_test_method_with_no_assertions_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $x = 1 + 1;
                }
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(d.iter().any(|x| x.identifier == "test.risky"), "got: {d:?}");
}

#[test]
fn a_test_method_with_an_assertion_is_not_flagged() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $this->assertTrue(true);
                }
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn an_assertion_nested_inside_a_conditional_still_counts() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    if (true) {
                        $this->assertTrue(true);
                    }
                }
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn expect_exception_alone_is_enough() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $this->expectException(\Exception::class);
                    throw new \Exception();
                }
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn mark_test_skipped_alone_is_enough() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $this->markTestSkipped('not yet');
                }
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn a_static_assert_call_also_counts() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    self::assertTrue(true);
                }
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_test_method_is_never_checked() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function helperMethod() {
                    $x = 1 + 1;
                }
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn a_test_prefixed_method_on_a_non_testcase_class_is_never_checked() {
    let d = diagnostics_for_with(
        r#"<?php
            class NotATest {
                public function testSomething() {
                    $x = 1 + 1;
                }
            }
        "#,
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn an_abstract_test_method_is_never_checked() {
    let d = diagnostics_for_with(
        &with_test_case(
            r#"
            abstract class MyTest extends \PHPUnit\Framework\TestCase {
                abstract public function testSomething();
            }
        "#,
        ),
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn a_pest_it_closure_with_no_assertions_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function it($description, $callback) {}
            it('does something', function () {
                $x = 1 + 1;
            });
        "#,
        |r| r.check_risky_tests = true,
    );
    assert!(d.iter().any(|x| x.identifier == "test.risky"), "got: {d:?}");
}

#[test]
fn a_pest_test_closure_using_expect_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test($description, $callback) {}
            test('does something', function () {
                expect(1 + 1)->toBe(2);
            });
        "#,
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn a_pest_describe_closure_is_never_checked_as_a_test_body() {
    let d = diagnostics_for_with(
        r#"<?php
            function describe($description, $callback) {}
            describe('a group', function () {
                $x = 1 + 1;
            });
        "#,
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}

#[test]
fn a_pest_before_each_hook_is_never_checked_as_a_test_body() {
    let d = diagnostics_for_with(
        r#"<?php
            function beforeEach($callback) {}
            beforeEach(function () {
                $x = 1 + 1;
            });
        "#,
        |r| r.check_risky_tests = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "test.risky"),
        "got: {d:?}"
    );
}
