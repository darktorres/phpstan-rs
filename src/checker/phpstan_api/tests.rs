//! Tests for Phase 40's `PHPStan\*`-reference checker.
//!
//! Each test supplies two separate source snippets, parsed and indexed
//! independently - `stub_src` (a small fake `PHPStan\...`-namespaced
//! declaration, standing in for a real `vendor/phpstan/phpstan/src/...`
//! file) and `consumer_src` (the code actually under test) - exactly
//! mirroring how these always live in genuinely separate files in any
//! real corpus (a project's own `src/MyRule.php` is never the same file
//! as `vendor/phpstan/phpstan/src/Rules/Rule.php`). Both parse trees are
//! merged for `Registry::build` (so the consumer's references resolve
//! against the stub's real declarations), but `check_phpstan_api_usage`
//! itself only ever walks `consumer_src`'s own parse tree - keeping each
//! test to a single `namespace` declaration per source string sidesteps
//! this module's own accepted "one global, last-namespace-wins
//! `NameContext` per walk" simplification (see its own doc comment),
//! which - like `Checker::check_program`'s identical simplification -
//! would otherwise mislabel declarations from an earlier `namespace`
//! block in a single multi-namespace test file.

use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::Registry;

fn parse(src: &str) -> Vec<Stmt> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    Parser::new(tokens).parse_program().unwrap()
}

fn diagnostics_for(stub_src: &str, consumer_src: &str) -> Vec<Diagnostic> {
    let stub = parse(stub_src);
    let consumer = parse(consumer_src);
    let mut combined = stub.clone();
    combined.extend(consumer.clone());
    let registry = Registry::build(&combined);
    check_phpstan_api_usage(&consumer, &registry)
}

#[test]
fn instantiating_a_non_api_phpstan_class_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class Rule {}
        "#,
        r#"<?php
        namespace App;
        function f() {
            $r = new \PHPStan\Rules\Rule();
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("instantiating PHPStan\\Rules\\Rule")
    );
    assert!(diags[0].message.contains("missing @api"));
    assert_eq!(diags[0].level, 0);
}

#[test]
fn instantiating_an_api_tagged_phpstan_class_is_not_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        /** @api */
        class Rule {}
        "#,
        r#"<?php
        namespace App;
        function f() {
            $r = new \PHPStan\Rules\Rule();
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn a_reference_outside_the_phpstan_namespace_is_never_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class Rule {}
        function f() {
            $r = new Rule();
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn the_bundled_phpdocparser_and_betterreflection_sub_namespaces_are_exempt() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\PhpDocParser;
        class Lexer {}

        namespace PHPStan\BetterReflection;
        class Reflector {}
        "#,
        r#"<?php
        namespace App;
        function f() {
            new \PHPStan\PhpDocParser\Lexer();
            new \PHPStan\BetterReflection\Reflector();
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn a_static_call_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class Helper {
            public static function check() {}
        }
        "#,
        r#"<?php
        namespace App;
        function f() {
            \PHPStan\Rules\Helper::check();
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Rules\\Helper::check()")
    );
}

#[test]
fn a_class_const_fetch_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class Helper {
            const FOO = 1;
        }
        "#,
        r#"<?php
        namespace App;
        function f() {
            echo \PHPStan\Rules\Helper::FOO;
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("accessing PHPStan\\Rules\\Helper::FOO")
    );
}

#[test]
fn referencing_class_string_via_double_colon_class_is_never_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        interface Rule {}
        "#,
        r#"<?php
        namespace App;
        function f() {
            $x = \PHPStan\Rules\Rule::class;
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn an_instanceof_check_against_a_non_api_class_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class Rule {}
        "#,
        r#"<?php
        namespace App;
        function f($x) {
            if ($x instanceof \PHPStan\Rules\Rule) {
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("instanceof PHPStan\\Rules\\Rule"));
}

#[test]
fn extending_a_non_api_phpstan_class_is_flagged_as_not_covered() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class BaseRule {}
        "#,
        r#"<?php
        namespace App;
        class MyRule extends \PHPStan\Rules\BaseRule {}
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("extending PHPStan\\Rules\\BaseRule")
    );
    assert!(diags[0].message.contains("missing @api"));
}

#[test]
fn extending_a_plain_api_phpstan_class_is_not_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        /** @api */
        class BaseRule {}
        "#,
        r#"<?php
        namespace App;
        class MyRule extends \PHPStan\Rules\BaseRule {}
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn implementing_an_api_do_not_implement_interface_is_flagged_distinctly() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        /**
         * @api
         * @api-do-not-implement
         */
        interface Scope {}
        "#,
        r#"<?php
        namespace App;
        class MyScope implements \PHPStan\Analyser\Scope {}
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("implementing PHPStan\\Analyser\\Scope")
    );
    assert!(diags[0].message.contains("@api-do-not-implement"));
}

#[test]
fn calling_a_method_of_an_api_do_not_implement_interface_is_still_fine() {
    // `@api-do-not-implement` only blocks extends/implements/use - the
    // interface's own methods are still fully `@api`-covered to *call*.
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        /**
         * @api
         * @api-do-not-implement
         */
        interface Scope {
            public function getType();
        }
        "#,
        r#"<?php
        namespace App;
        function f(\PHPStan\Analyser\Scope $scope) {
            $scope->getType();
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn using_a_non_api_phpstan_trait_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        trait RuleHelperTrait {}
        "#,
        r#"<?php
        namespace App;
        class MyRule {
            use \PHPStan\Rules\RuleHelperTrait;
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("using trait PHPStan\\Rules\\RuleHelperTrait")
    );
}

#[test]
fn calling_a_method_through_a_typed_parameter_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        class Scope {
            public function getType() {}
        }
        "#,
        r#"<?php
        namespace PHPStan\Rules;
        class SomeRule {
            public function processNode(\PHPStan\Analyser\Scope $scope) {
                $scope->getType();
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Analyser\\Scope::getType()")
    );
    assert_eq!(diags[0].location, "PHPStan\\Rules\\SomeRule::processNode");
}

#[test]
fn calling_a_method_through_a_typed_api_parameter_is_not_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        /** @api */
        class Scope {
            public function getType() {}
        }
        "#,
        r#"<?php
        namespace App;
        function f(\PHPStan\Analyser\Scope $scope) {
            $scope->getType();
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn a_method_call_through_an_untyped_parameter_is_never_flagged() {
    // No native type hint at all means this checker has no reliable signal
    // for what `$scope` is - guarding against a false positive here, not a
    // missed detection, is the point of this test.
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        class Scope {
            public function getType() {}
        }
        "#,
        r#"<?php
        namespace App;
        function f($scope) {
            $scope->getType();
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn a_typed_parameter_captured_by_a_closure_use_clause_still_gets_checked() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        class Scope {
            public function getType() {}
        }
        "#,
        r#"<?php
        namespace App;
        function f(\PHPStan\Analyser\Scope $scope) {
            $cb = function () use ($scope) {
                $scope->getType();
            };
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Analyser\\Scope::getType()")
    );
}

#[test]
fn a_typed_parameter_not_captured_by_a_closure_use_clause_is_not_checked_inside_it() {
    // Real PHP: a plain (non-arrow) closure only sees an outer variable it
    // explicitly lists in `use (...)` - `$scope` here is simply undefined
    // inside the closure body, not the same `$scope` as the enclosing
    // function's parameter.
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        class Scope {
            public function getType() {}
        }
        "#,
        r#"<?php
        namespace App;
        function f(\PHPStan\Analyser\Scope $scope) {
            $cb = function () {
                $scope->getType();
            };
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn an_arrow_function_implicitly_inherits_the_enclosing_typed_parameter() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        class Scope {
            public function getType() {}
        }
        "#,
        r#"<?php
        namespace App;
        function f(\PHPStan\Analyser\Scope $scope) {
            $cb = fn() => $scope->getType();
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Analyser\\Scope::getType()")
    );
}

#[test]
fn a_reference_to_an_unresolved_phpstan_shaped_class_is_not_flagged() {
    // Nothing in the registry actually declares `PHPStan\NoSuchClass` -
    // an undefined-class diagnostic (a different, more meaningful check)
    // is the right signal here, not this one, so this checker stays quiet
    // rather than double-reporting.
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        function f() {
            new \PHPStan\NoSuchClass();
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}
