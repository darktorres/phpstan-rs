//! Tests for the `PHPStan\*`-reference checker.
//!
//! Each test supplies two separate source snippets, parsed and indexed
//! independently - `stub_src` (a small fake `PHPStan\...`-namespaced
//! declaration, standing in for a `vendor/phpstan/phpstan/src/...` file)
//! and `consumer_src` (the code actually under test) - exactly mirroring
//! how these always live in genuinely separate files in practice (a
//! project's own `src/MyRule.php` is never the same file as
//! `vendor/phpstan/phpstan/src/Rules/Rule.php`). Both parse trees are
//! merged for `Registry::build` (so the consumer's references resolve
//! against the stub's declarations), but `check_phpstan_api_usage`
//! itself only ever walks `consumer_src`'s own parse tree - keeping each
//! test to a single `namespace` declaration per source string sidesteps
//! this module's accepted "one global, last-namespace-wins `NameContext`
//! per walk" simplification (see its own doc comment), which - like
//! `Checker::check_program`'s identical simplification - would otherwise
//! mislabel declarations from an earlier `namespace` block in a single
//! multi-namespace test file.

use super::*;
use crate::registry::Registry;

fn parse(src: &str) -> syntax::ParsedFile {
    syntax::parse(src).unwrap()
}

fn diagnostics_for(stub_src: &str, consumer_src: &str) -> Vec<Diagnostic> {
    let source = format!("{stub_src}?>\n{consumer_src}");
    let consumer_start = stub_src.len() as u32 + 3;
    let combined = parse(&source);
    let registry = Registry::build(&combined.arenas, combined.statements());
    let consumer_stmts: Vec<_> = combined
        .statements()
        .iter()
        .copied()
        .filter(|id| {
            combined.arenas.stmt(*id).is_some_and(|_| {
                combined
                    .arenas
                    .stmt_span(*id)
                    .is_some_and(|span| span.start >= consumer_start)
            })
        })
        .collect();
    check_phpstan_api_usage(
        &combined.arenas,
        &consumer_stmts,
        &syntax::span::LineIndex::default(),
        &registry,
        None,
    )
}

#[test]
fn a_static_call_to_parent_on_a_non_api_base_is_not_flagged_absent_vendor_evidence() {
    // The calling scope (`MyRule`) is itself `PHPStan\*`-namespaced, so
    // `ApiRuleHelper::isPhpStanCode` additionally requires
    // vendor-directory evidence for the target - this string-only
    // harness (`Registry::build`, no real file paths) never has any,
    // matching PHPStan's own no-`declaringFile` answer.
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
    assert!(
        !diags.iter().any(|d| d
            .message
            .contains("calling PHPStan\\Rules\\BaseRule::check()")),
        "unexpected diagnostics: {diags:?}"
    );
}

#[test]
fn a_static_call_to_self_on_a_non_api_class_is_not_flagged_absent_vendor_evidence() {
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
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn a_static_call_to_static_on_a_non_api_class_is_not_flagged_absent_vendor_evidence() {
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
    // Mirrors PHPStan's `ApiInstanceofRule`: still flags an `@api`-tagged
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

// `ApiInstanceofTypeRule`, entirely independent of `@api` status - a
// deprecated-since-forever idiom (`instanceof StringType` instead of
// `Type::isString()`) PHPStan flags unconditionally on its compiled
// `#[InstanceofDeprecated]` table.

// An interface's `extends` gets `phpstanApi.interface` (mirrors
// PHPStan's `ApiInterfaceExtendsRule`), not the class-side
// `phpstanApi.class` a plain class's `extends` reports (mirrors
// `ApiClassExtendsRule`).

// `RuntimeReflectionFunctionRule`/`RuntimeReflectionInstantiationRule`/
// `OldPhpParser4ClassRule` - all three gate on `class_implements_phpstan_
// interface`, so every positive test below declares its enclosing class
// as `implements \PHPStan\Rules\Rule` (a bare, undeclared interface
// reference is enough: `Registry::class_has_phpstan_ancestry` only needs
// the *name* to resolve into the `PHPStan\` namespace, not a real
// registry entry for it - `Registry::all_ancestor_names` walks
// `ClassReflection::implements`, populated straight from the raw, resolved
// `implements` clause regardless of whether the interface itself was
// ever declared anywhere in this test's own tiny source).

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

// `PhpStanNamespaceIn3rdPartyPackageRule` - the one rule of the three
// that needs a real file path on disk (`find_composer_package_
// name`'s own upward walk), so these tests build their own tiny
// temporary directory tree rather than using `diagnostics_for` (which
// always passes `None`).
mod namespace_in_3rd_party_package {
    use std::path::{Path, PathBuf};

    use super::super::check_phpstan_api_usage;
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

    /// Checks `src` as the file `php_path` of the project rooted at
    /// `root`, with package ownership read from `root` the way a session
    /// reads it.
    fn diagnostics_for_file(
        root: &TempDir,
        php_path: &Path,
        src: &str,
    ) -> Vec<diagnostics::Diagnostic> {
        let program = syntax::parse(src).unwrap();
        let registry = Registry::build(&program.arenas, program.statements());
        let absolute_root = std::fs::canonicalize(&root.0).unwrap();
        let packages = composer::PackageIndex::new(
            composer::PackageContext::load(&absolute_root, &absolute_root)
                .manifest
                .into_iter()
                .collect(),
        );
        let file = absolute_root.join(php_path.strip_prefix(&root.0).unwrap());
        let package = packages.owner(&file).and_then(|o| packages.name(o));
        check_phpstan_api_usage(
            &program.arenas,
            program.statements(),
            &syntax::span::LineIndex::default(),
            &registry,
            Some(package),
        )
    }

    #[test]
    fn a_phpstan_namespace_declared_outside_any_phpstan_slash_package_is_flagged() {
        let dir = TempDir::new("not_phpstan_pkg");
        dir.write("composer.json", r#"{"name": "acme/some-extension"}"#);
        let php_path = dir.write("src/MyRule.php", "");
        let diags = diagnostics_for_file(
            &dir,
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
            &dir,
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
            &dir,
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
            &dir,
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
