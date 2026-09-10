//! Phase 40.C: `phpstanApi.method`/`phpParser.nodeConnectingAttribute` -
//! folded into the main `Checker` pass from `phpstan_api.rs`'s own
//! previously-separate `Walker`, which already covers every *other*
//! PHPStan-API-usage check (`src/checker/phpstan_api/tests.rs`). General
//! receiver-type inference here recognizes every receiver shape the main
//! checker's own `find_method_receiver` does, not just a directly-typed
//! parameter or `$this->prop` - the previous walker's own narrower
//! coverage is what these tests replace.
//!
//! Every fixture declares both the fake `PHPStan\...`-namespaced stub and
//! the consuming code in one source string (unlike `phpstan_api/tests.rs`'s
//! own two-source-string split) - the main checker's `diagnostics_for`
//! only ever runs against one parsed/indexed program, which (unlike that
//! other module's own standalone `Registry::build` call) also auto-embeds
//! the real vendored `phpstorm-stubs`/`phpstan-src` corpus for stdlib-
//! function test realism elsewhere. `PHPStan\FakeNs\Widget` (not a real
//! PHPStan namespace/class) is used throughout instead of a real one like
//! `PHPStan\Analyser\Scope` specifically to avoid colliding with that
//! auto-embedded corpus's own real declaration of it - confirmed the hard
//! way that a same-named fake redeclaration merges unpredictably with the
//! real one and silently defeats what a test meant to check.

use super::helpers::*;
use crate::Diagnostic;

fn diagnostics_for_phpstan_api(src: &str) -> Vec<Diagnostic> {
    diagnostics_for_with(src, |r| r.phpstan_api_check_enabled = true)
}

#[test]
fn calling_a_method_through_a_typed_parameter_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace PHPStan\Rules;
        class SomeRule {
            public function processNode(\PHPStan\FakeNs\Widget $scope) {
                $scope->getType();
            }
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.identifier == "phpstanApi.method"
            && d.message
                .contains("calling PHPStan\\FakeNs\\Widget::getType()")),
        "got: {diags:?}"
    );
}

#[test]
fn calling_a_method_through_a_typed_api_parameter_is_not_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        /** @api */
        class Widget {
            public function getType() {}
        }
        namespace App;
        function f(\PHPStan\FakeNs\Widget $scope) {
            $scope->getType();
        }
        "#,
    );
    assert!(
        !diags.iter().any(|d| d.identifier == "phpstanApi.method"),
        "unexpected diagnostics: {diags:?}"
    );
}

#[test]
fn a_typed_parameter_captured_by_a_closure_use_clause_still_gets_checked() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace App;
        function f(\PHPStan\FakeNs\Widget $scope) {
            $cb = function () use ($scope) {
                $scope->getType();
            };
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.identifier == "phpstanApi.method"
            && d.message
                .contains("calling PHPStan\\FakeNs\\Widget::getType()")),
        "got: {diags:?}"
    );
}

#[test]
fn an_arrow_function_implicitly_inherits_the_enclosing_typed_parameter() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace App;
        function f(\PHPStan\FakeNs\Widget $scope) {
            $cb = fn() => $scope->getType();
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.identifier == "phpstanApi.method"
            && d.message
                .contains("calling PHPStan\\FakeNs\\Widget::getType()")),
        "got: {diags:?}"
    );
}

#[test]
fn calling_a_method_through_a_promoted_property_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace PHPStan\Rules;
        class SomeRule {
            public function __construct(private \PHPStan\FakeNs\Widget $scope) {}
            public function processNode() {
                $this->scope->getType();
            }
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.identifier == "phpstanApi.method"
            && d.message
                .contains("calling PHPStan\\FakeNs\\Widget::getType()")),
        "got: {diags:?}"
    );
}

#[test]
fn calling_a_method_through_an_ordinary_typed_property_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace PHPStan\Rules;
        class SomeRule {
            private \PHPStan\FakeNs\Widget $scope;
            public function processNode() {
                $this->scope->getType();
            }
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.identifier == "phpstanApi.method"
            && d.message
                .contains("calling PHPStan\\FakeNs\\Widget::getType()")),
        "got: {diags:?}"
    );
}

#[test]
fn calling_a_method_through_a_promoted_api_property_is_not_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        /** @api */
        class Widget {
            public function getType() {}
        }
        namespace App;
        class SomeRule {
            public function __construct(private \PHPStan\FakeNs\Widget $scope) {}
            public function processNode() {
                $this->scope->getType();
            }
        }
        "#,
    );
    assert!(
        !diags.iter().any(|d| d.identifier == "phpstanApi.method"),
        "unexpected diagnostics: {diags:?}"
    );
}

#[test]
fn the_check_is_silent_when_phpstan_api_check_enabled_is_off() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace PHPStan\Rules;
        class SomeRule {
            public function processNode(\PHPStan\FakeNs\Widget $scope) {
                $scope->getType();
            }
        }
        "#,
    );
    assert!(
        !diags.iter().any(|d| d.identifier == "phpstanApi.method"),
        "unexpected diagnostics: {diags:?}"
    );
}

// A real receiver shape the old, syntactically-typed-only walker could
// never recognize at all: a method call chained straight off another
// method call's own return type, resolved purely through general
// inference (`find_method_receiver`) - not a directly-typed parameter or
// `$this->prop`.
#[test]
fn calling_a_method_through_a_chained_call_return_type_is_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType(): self { return $this; }
        }
        namespace PHPStan\Rules;
        class SomeRule {
            public function processNode(\PHPStan\FakeNs\Widget $scope) {
                $scope->getType()->getType();
            }
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .filter(|d| d.identifier == "phpstanApi.method")
            .count()
            >= 2,
        "got: {diags:?}"
    );
}

// Phase 40.F: `NodeConnectingVisitorAttributesRule` - `$node->getAttribute
// ('parent'|'previous'|'next')` no longer works, only inside a class
// implementing a `PHPStan\*` interface, only on a real `PhpParser\Node`
// (sub)type receiver.

#[test]
fn getting_the_parent_node_attribute_inside_a_phpstan_rule_class_is_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('parent');
            }
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "phpParser.nodeConnectingAttribute"
                && d.message
                    .contains("Node attribute 'parent' is no longer available")),
        "got: {diags:?}"
    );
}

#[test]
fn getting_an_unrelated_node_attribute_is_never_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('startLine');
            }
        }
        "#,
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.identifier == "phpParser.nodeConnectingAttribute"),
        "unexpected diagnostics: {diags:?}"
    );
}

#[test]
fn getting_the_parent_node_attribute_outside_any_phpstan_rule_class_is_never_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace App;
        class MyRule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('parent');
            }
        }
        "#,
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.identifier == "phpParser.nodeConnectingAttribute"),
        "unexpected diagnostics: {diags:?}"
    );
}

#[test]
fn getting_the_parent_attribute_on_a_real_node_subtype_is_flagged() {
    let diags = diagnostics_for_phpstan_api(
        r#"<?php
        namespace PhpParser {
            class Node {}
        }
        namespace PhpParser\Node\Stmt {
            class ClassMethod extends \PhpParser\Node {}
        }
        namespace App {
            class MyRule implements \PHPStan\Rules\Rule {
                public function check(\PhpParser\Node\Stmt\ClassMethod $node) {
                    $node->getAttribute('previous');
                }
            }
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "phpParser.nodeConnectingAttribute"),
        "got: {diags:?}"
    );
}
