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
    check_phpstan_api_usage(&consumer, &registry, None)
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
fn instantiating_an_api_tagged_phpstan_class_is_still_flagged_by_default() {
    // Phase 40.2: real `ApiInstantiationRule` flags instantiation
    // unconditionally by default - a class-level `@api` does NOT exempt
    // it (extensions should use dependency injection, not direct
    // instantiation) unless the class's own, non-inherited constructor
    // is itself `@api`-tagged (see the next test).
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
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "phpstanApi.constructor"),
        "got: {diags:?}"
    );
}

#[test]
fn instantiating_a_class_with_its_own_api_tagged_constructor_is_not_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        /** @api */
        class Rule {
            /** @api */
            public function __construct() {}
        }
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
fn instantiating_a_class_with_only_an_inherited_api_constructor_is_still_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class BaseRule {
            /** @api */
            public function __construct() {}
        }
        /** @api */
        class Rule extends BaseRule {}
        "#,
        r#"<?php
        namespace App;
        function f() {
            $r = new \PHPStan\Rules\Rule();
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "phpstanApi.constructor"),
        "got: {diags:?}"
    );
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

// Phase 40.B: a static call's own real *declaring* class (which can
// differ from the referenced class under inheritance), and the method's
// own docblock `@api` exemption - both real gaps confirmed against real
// `ApiStaticCallRule::isCovered()`.

#[test]
fn a_static_call_to_an_inherited_method_checks_the_declaring_classs_own_api_status() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class BaseHelper {
            public static function check() {}
        }
        /** @api */
        class Helper extends BaseHelper {}
        "#,
        r#"<?php
        namespace App;
        function f() {
            \PHPStan\Rules\Helper::check();
        }
        "#,
    );
    // Helper itself is @api, but check() is really declared on the
    // non-@api BaseHelper - real PHPStan flags this (referenced-class-
    // only checking would have wrongly stayed silent).
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Rules\\BaseHelper::check()")
    );
}

#[test]
fn a_static_call_to_a_method_with_its_own_api_doc_is_never_flagged_even_on_a_non_api_class() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class Helper {
            /** @api */
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
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn a_static_call_to_an_api_classs_own_constructor_is_still_flagged() {
    // Real PHPStan's own explicit exemption: a class-level @api doesn't
    // cover __construct - only the constructor's own @api would.
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        /** @api */
        class Helper {
            public function __construct() {}
        }
        "#,
        r#"<?php
        namespace App;
        function f() {
            \PHPStan\Rules\Helper::__construct();
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Rules\\Helper::__construct()")
    );
}

// Phase 40.D: `self`/`static`/`parent` pseudo-class resolution -
// `NameContext::resolve` has no special-casing for any of the three, and
// this standalone `Walker` (unlike the main `Checker`) previously had no
// "current class" state at all to resolve them against.

#[test]
fn a_static_call_to_parent_on_a_non_api_base_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        class BaseRule {
            public static function check() {}
        }
        "#,
        r#"<?php
        namespace PHPStan\Rules;
        class MyRule extends BaseRule {
            public static function run() {
                parent::check();
            }
        }
        "#,
    );
    // Also, separately, flags `extends BaseRule` itself (Phase 40.A) -
    // both are real, independent findings; this test cares only about
    // the `parent::check()` one.
    assert!(
        diags.iter().any(|d| d
            .message
            .contains("calling PHPStan\\Rules\\BaseRule::check()")),
        "got: {diags:?}"
    );
}

#[test]
fn a_static_call_to_self_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace App;
        function unused() {}
        "#,
        r#"<?php
        namespace PHPStan\Rules;
        class MyRule {
            public static function check() {}
            public static function run() {
                self::check();
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Rules\\MyRule::check()")
    );
}

#[test]
fn a_static_call_to_static_on_a_non_api_class_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace App;
        function unused() {}
        "#,
        r#"<?php
        namespace PHPStan\Rules;
        class MyRule {
            public static function check() {}
            public static function run() {
                static::check();
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0]
            .message
            .contains("calling PHPStan\\Rules\\MyRule::check()")
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
    assert_eq!(diags[0].identifier, "phpstanApi.class");
}

#[test]
fn an_instanceof_check_against_an_api_class_is_still_flagged_as_an_assumption() {
    // Phase 40.3: real `ApiInstanceofRule` still flags an `@api`-tagged
    // class's instanceof - just with a different identifier/message
    // warning the assumption itself might break in a minor version.
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        /** @api */
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
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "phpstanApi.instanceofAssumption"),
        "got: {diags:?}"
    );
}

#[test]
fn an_instanceof_check_against_an_api_interface_is_never_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        /** @api */
        interface Rule {}
        "#,
        r#"<?php
        namespace App;
        function f($x) {
            if ($x instanceof \PHPStan\Rules\Rule) {
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// Phase 40.E: `ApiInstanceofTypeRule`, entirely independent of `@api`
// status - a real, deprecated-since-forever idiom (`instanceof StringType`
// instead of `Type::isString()`) real PHPStan flags unconditionally on its
// own compiled `#[InstanceofDeprecated]` table.

#[test]
fn an_instanceof_check_against_a_deprecated_type_class_is_flagged_with_its_replacement_hint() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Type;
        /** @api */
        class StringType {}
        "#,
        r#"<?php
        namespace App;
        function f($x) {
            if ($x instanceof \PHPStan\Type\StringType) {
            }
        }
        "#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "phpstanApi.instanceofType"
                && d.message.contains("Use Type::isString() instead")),
        "got: {diags:?}"
    );
}

#[test]
fn an_instanceof_check_against_an_accessory_type_subtype_is_never_flagged_as_deprecated() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Type\Accessory;
        /** @api */
        interface AccessoryType {}
        /** @api */
        class AccessoryNonEmptyStringType implements AccessoryType {}
        "#,
        r#"<?php
        namespace App;
        function f($x) {
            if ($x instanceof \PHPStan\Type\Accessory\AccessoryNonEmptyStringType) {
            }
        }
        "#,
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.identifier == "phpstanApi.instanceofType"),
        "got: {diags:?}"
    );
}

#[test]
fn an_instanceof_check_against_a_deprecated_type_class_inside_a_type_traverser_callable_is_never_flagged()
 {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Type;
        /** @api */
        class StringType {}
        interface TypeTraverserCallable {}
        "#,
        r#"<?php
        namespace App;
        class MyCallback implements \PHPStan\Type\TypeTraverserCallable {
            public function run($x) {
                if ($x instanceof \PHPStan\Type\StringType) {
                }
            }
        }
        "#,
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.identifier == "phpstanApi.instanceofType"),
        "got: {diags:?}"
    );
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

// Phase 40.A: an interface's own `extends` gets `phpstanApi.interface`
// (real `ApiInterfaceExtendsRule`), not the class-side `phpstanApi.class`
// a plain class's own `extends` reports (real `ApiClassExtendsRule`).

#[test]
fn an_interface_extending_a_non_api_phpstan_interface_is_flagged_as_phpstan_api_interface() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        interface BaseRule {}
        "#,
        r#"<?php
        namespace App;
        interface MyRule extends \PHPStan\Rules\BaseRule {}
        "#,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].identifier, "phpstanApi.interface");
}

#[test]
fn a_class_extending_a_non_api_phpstan_class_still_uses_phpstan_api_class() {
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
    assert_eq!(diags[0].identifier, "phpstanApi.class");
}

#[test]
fn extending_phpstans_own_mutating_scope_is_never_flagged() {
    // Phase 40.A: real `ApiClassExtendsRule`'s own narrow, explicit
    // carve-out (`processNode`'s early return on `MutatingScope::class`).
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Analyser;
        class MutatingScope {}
        "#,
        r#"<?php
        namespace App;
        class MyScope extends \PHPStan\Analyser\MutatingScope {}
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
fn using_an_api_tagged_phpstan_trait_is_still_flagged_unconditionally() {
    // Phase 40.4: real `ApiTraitUseRule` grants no `@api` escape hatch at
    // all, unlike extends/implements.
    let diags = diagnostics_for(
        r#"<?php
        namespace PHPStan\Rules;
        /** @api */
        trait RuleHelperTrait {}
        "#,
        r#"<?php
        namespace App;
        class MyRule {
            use \PHPStan\Rules\RuleHelperTrait;
        }
        "#,
    );
    assert!(
        diags.iter().any(|d| d.identifier == "phpstanApi.trait"),
        "got: {diags:?}"
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

#[test]
fn calling_a_method_through_a_promoted_property_on_a_non_api_class_is_flagged() {
    // Phase 40.1: `$this->scope->getType()` through a constructor-
    // promoted property - the one extra receiver shape this walker's own
    // minimal local type tracker (`class_prop_types`) now recognizes,
    // beyond the pre-existing bare-`$variable` case.
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
            public function __construct(private \PHPStan\Analyser\Scope $scope) {}
            public function processNode() {
                $this->scope->getType();
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
fn calling_a_method_through_an_ordinary_typed_property_on_a_non_api_class_is_flagged() {
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
            private \PHPStan\Analyser\Scope $scope;
            public function processNode() {
                $this->scope->getType();
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
}

#[test]
fn calling_a_method_through_a_promoted_api_property_is_not_flagged() {
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
        class SomeRule {
            public function __construct(private \PHPStan\Analyser\Scope $scope) {}
            public function processNode() {
                $this->scope->getType();
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// Phase 40.6: `RuntimeReflectionFunctionRule`/`RuntimeReflectionInstantiationRule`/
// `OldPhpParser4ClassRule` - all three gate on `class_implements_phpstan_
// interface`, so every positive test below declares its enclosing class
// as `implements \PHPStan\Rules\Rule` (a bare, undeclared interface
// reference is enough: `class_implements_phpstan_interface` only needs
// the *name* to resolve into the `PHPStan\` namespace, not a real
// registry entry for it - `Registry::all_ancestor_names` walks
// `ClassInfo::implements`, populated straight from the raw, resolved
// `implements` clause regardless of whether the interface itself was
// ever declared anywhere in this test's own tiny source).

#[test]
fn calling_is_a_inside_a_phpstan_rule_class_is_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check($x) {
                is_a($x, 'Foo');
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert!(diags[0].message.contains("Function is_a()"));
    assert_eq!(diags[0].identifier, "phpstanApi.runtimeReflection");
}

#[test]
fn calling_is_a_outside_any_phpstan_rule_class_is_not_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule {
            public function check($x) {
                is_a($x, 'Foo');
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn calling_an_unrelated_function_inside_a_phpstan_rule_class_is_not_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check($x) {
                strlen($x);
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// Phase 40.F: `NodeConnectingVisitorAttributesRule` - `$node->getAttribute
// ('parent'|'previous'|'next')` no longer works, only inside a class
// implementing a `PHPStan\*` interface, only on a real `PhpParser\Node`
// (sub)type receiver.

#[test]
fn getting_the_parent_node_attribute_inside_a_phpstan_rule_class_is_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('parent');
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert!(
        diags[0]
            .message
            .contains("Node attribute 'parent' is no longer available")
    );
    assert_eq!(diags[0].identifier, "phpParser.nodeConnectingAttribute");
}

#[test]
fn getting_an_unrelated_node_attribute_is_never_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('startLine');
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn getting_the_parent_node_attribute_outside_any_phpstan_rule_class_is_never_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule {
            public function check(\PhpParser\Node $node) {
                $node->getAttribute('parent');
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn getting_the_parent_attribute_on_a_real_node_subtype_is_flagged() {
    let diags = diagnostics_for(
        r#"<?php
        namespace PhpParser {
            class Node {}
        }
        namespace PhpParser\Node\Stmt {
            class ClassMethod extends \PhpParser\Node {}
        }
        "#,
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check(\PhpParser\Node\Stmt\ClassMethod $node) {
                $node->getAttribute('previous');
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert_eq!(diags[0].identifier, "phpParser.nodeConnectingAttribute");
}

#[test]
fn instantiating_reflectionclass_inside_a_phpstan_rule_class_is_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check($x) {
                new \ReflectionClass($x);
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert!(diags[0].message.contains("Creating new ReflectionClass"));
    assert_eq!(diags[0].identifier, "phpstanApi.runtimeReflection");
}

#[test]
fn instantiating_an_unrelated_class_inside_a_phpstan_rule_class_is_not_flagged_by_this_rule() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check($x) {
                new \stdClass();
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn referencing_a_renamed_php_parser_4_class_via_new_is_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check() {
                new \PhpParser\Node\Scalar\LNumber(1);
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert!(
        diags[0]
            .message
            .contains("It has been renamed to PhpParser\\Node\\Scalar\\Int_ in PHP-Parser v5.")
    );
    assert_eq!(diags[0].identifier, "phpParser.classRenamed");
}

#[test]
fn referencing_a_renamed_php_parser_4_class_via_instanceof_is_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check($node) {
                $node instanceof \PhpParser\Node\Stmt\UseUse;
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert!(
        diags[0]
            .message
            .contains("It has been renamed to PhpParser\\Node\\UseItem in PHP-Parser v5.")
    );
}

#[test]
fn referencing_a_renamed_php_parser_4_class_via_class_constant_is_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check() {
                $x = \PhpParser\Node\Scalar\DNumber::class;
            }
        }
        "#,
    );
    assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    assert!(
        diags[0]
            .message
            .contains("It has been renamed to PhpParser\\Node\\Scalar\\Float_ in PHP-Parser v5.")
    );
}

#[test]
fn referencing_a_renamed_php_parser_4_class_outside_a_phpstan_rule_class_is_not_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule {
            public function check() {
                new \PhpParser\Node\Scalar\LNumber(1);
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn referencing_a_still_current_php_parser_class_name_is_not_flagged() {
    let diags = diagnostics_for(
        "<?php",
        r#"<?php
        namespace App;
        class MyRule implements \PHPStan\Rules\Rule {
            public function check() {
                new \PhpParser\Node\Scalar\Int_(1);
            }
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// Phase 40.6: `PhpStanNamespaceIn3rdPartyPackageRule` - the one rule of
// the three that needs a real file path on disk (`find_composer_package_
// name`'s own upward walk), so these tests build their own tiny
// temporary directory tree rather than using `diagnostics_for` (which
// always passes `None`).
mod namespace_in_3rd_party_package {
    use std::path::{Path, PathBuf};

    use super::super::check_phpstan_api_usage;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::registry::Registry;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "phpstan_rs_phpstan_api_test_{label}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn write(&self, rel: &str, contents: &str) -> PathBuf {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn diagnostics_for_file(php_path: &Path, src: &str) -> Vec<crate::checker::Diagnostic> {
        let tokens = Lexer::new(src).tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let registry = Registry::build(&program);
        check_phpstan_api_usage(&program, &registry, Some(php_path))
    }

    #[test]
    fn a_phpstan_namespace_declared_outside_any_phpstan_slash_package_is_flagged() {
        let dir = TempDir::new("not_phpstan_pkg");
        dir.write("composer.json", r#"{"name": "acme/some-extension"}"#);
        let php_path = dir.write("src/MyRule.php", "");
        let diags = diagnostics_for_file(
            &php_path,
            r#"<?php
            namespace PHPStan\Rules;
            class MyRule {}
            "#,
        );
        assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
        assert!(
            diags[0]
                .message
                .contains("Declaring PHPStan namespace is not allowed in 3rd party packages.")
        );
        assert_eq!(diags[0].identifier, "phpstanApi.phpstanNamespace");
    }

    #[test]
    fn a_phpstan_namespace_declared_inside_a_real_phpstan_slash_package_is_not_flagged() {
        let dir = TempDir::new("real_phpstan_pkg");
        dir.write("composer.json", r#"{"name": "phpstan/phpstan"}"#);
        let php_path = dir.write("src/Rules/Rule.php", "");
        let diags = diagnostics_for_file(
            &php_path,
            r#"<?php
            namespace PHPStan\Rules;
            class Rule {}
            "#,
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn a_phpstan_namespace_declared_with_no_composer_json_anywhere_is_flagged() {
        let dir = TempDir::new("no_composer_json_at_all");
        let php_path = dir.write("src/MyRule.php", "");
        let diags = diagnostics_for_file(
            &php_path,
            r#"<?php
            namespace PHPStan\Rules;
            class MyRule {}
            "#,
        );
        assert_eq!(diags.len(), 1, "diagnostics: {diags:?}");
    }

    #[test]
    fn a_non_phpstan_namespace_is_never_flagged_regardless_of_package() {
        let dir = TempDir::new("non_phpstan_ns");
        dir.write("composer.json", r#"{"name": "acme/some-extension"}"#);
        let php_path = dir.write("src/MyRule.php", "");
        let diags = diagnostics_for_file(
            &php_path,
            r#"<?php
            namespace App;
            class MyRule {}
            "#,
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }
}

#[test]
fn a_property_access_on_something_other_than_this_is_not_checked_via_the_prop_types_path() {
    // Only `$this->prop` is recognized - a different receiver variable's
    // own property access isn't, matching this walker's own deliberately
    // narrow, syntactically-typed scope (no general receiver-type
    // inference).
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
            public function __construct(private \PHPStan\Analyser\Scope $scope) {}
        }
        function f(SomeRule $r) {
            $other = $r;
        }
        "#,
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}
