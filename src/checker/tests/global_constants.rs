//! `Registry::global_constants` - a bare `const`/`define()`-declared
//! global constant now resolves to its own inferred literal type
//! instead of unconditionally falling back to `Mixed`.
use super::helpers::*;

#[test]
fn a_define_call_registers_the_constants_own_literal_type() {
    let d = diagnostics_for(
        r#"<?php
            define('FOO', 'hello');
            function needsInt(int $x): void {}
            needsInt(FOO);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn a_define_call_with_a_compatible_argument_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            define('FOO', 42);
            function needsInt(int $x): void {}
            needsInt(FOO);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_top_level_const_registers_the_constants_own_literal_type() {
    let d = diagnostics_for(
        r#"<?php
            const FOO = 'hello';
            function needsInt(int $x): void {}
            needsInt(FOO);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn a_define_call_with_a_non_literal_name_is_never_registered() {
    let d = diagnostics_for(
        r#"<?php
            function name(): string { return 'FOO'; }
            define(name(), 42);
            function needsInt(int $x): void {}
            needsInt(FOO);
        "#,
    );
    assert!(
        d.iter().all(|x| x.identifier == "constant.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_defined_constant_is_never_namespace_relative() {
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            define('FOO', 'hello');
            function needsInt(int $x): void {}
            needsInt(FOO);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn a_namespaced_const_is_looked_up_relative_to_its_own_namespace() {
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            const FOO = 'hello';
            function needsInt(int $x): void {}
            needsInt(FOO);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn two_conflicting_define_sites_for_the_same_name_stay_unknown() {
    let d = diagnostics_for(
        r#"<?php
            function firstSite(): void { define('FOO', 0); }
            function secondSite(): void { define('FOO', 'hello'); }
            function needsInt(int $x): void {}
            needsInt(FOO);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn defining_true_false_or_null_never_shadows_their_own_keyword_meaning() {
    let d = diagnostics_for(
        r#"<?php
            define('true', (bool) 1, true);
            define('false', (bool) 0, true);
            define('null', null, true);
            function findOrFalse(): int|false { return \false; }
            function run(): void {
                $x = findOrFalse();
                if ($x !== \false) {
                    needsInt($x);
                }
            }
            function needsInt(int $x): void {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn defining_a_magic_constant_never_shadows_it_with_a_stub_placeholder() {
    let d = diagnostics_for(
        r#"<?php
            define('__DIR__', '', true);
            define('__LINE__', 0, true);
            /** @param non-empty-string $x */
            function needsNonEmptyString($x): void {}
            function run(): void {
                needsNonEmptyString(__DIR__);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// PHP_INT_MIN is registered with `Type::Int` directly rather than via
// ordinary literal inference, since its raw literal overflows i64 and
// would otherwise mistype as Float - and unlike a plain
// `global_constant_type` miss (which just falls back to `Mixed`), an
// unregistered constant is indistinguishable from "this constant
// doesn't exist" for the `use const` existence check (see
// `symbol_existence.rs`), which only ever sees whether a name made it
// into `global_constants` at all.
#[test]
fn php_int_min_is_registered_as_a_plain_int_not_the_overflowing_float_literal() {
    let d = diagnostics_for(
        r#"<?php
            define('PHP_INT_MIN', -9223372036854775808);
            define('PHP_INT_MAX', 9223372036854775807);
            function run(): int {
                return random_int(PHP_INT_MIN, PHP_INT_MAX);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dynamic_define_site_poisons_a_literal_one_regardless_of_order() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                public int $verbosity = 0;
            }
            function withDynamicValue(Config $c): void { define('FOO', $c->verbosity); }
            function withLiteralValue(): void { define('FOO', 0); }
            function needsString(string $x): void {}
            needsString(FOO);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dynamic_fetch_with_a_literal_name_resolves_like_a_static_one() {
    let d = diagnostics_for_with(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Config { const LEVEL = 3; }
            function f(): mixed {
                $name = 'LEVEL';
                assertType('3', Config::{$name});
                return Config::{'MISSING'};
            }
        "#,
        |r| r.php_version = Some((8, 3)),
    );
    let ids: Vec<&str> = d.iter().map(|x| x.identifier.as_str()).collect();
    assert_eq!(ids, vec!["classConstant.notFound"], "{d:?}");
}

#[test]
fn a_use_const_import_resolves_an_unqualified_constant_reference() {
    let d = diagnostics_for(
        r#"<?php
            namespace Bar {
                const ZED = 5;
                function zed(): int { return 1; }
            }
            namespace Foo {
                use const Bar\ZED;
                use function Bar\zed;
                use function PHPStan\Testing\assertType;
                function run(): void {
                    assertType('5', ZED);
                    assertType('int', zed());
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_use_function_import_never_answers_a_constant_reference() {
    let d = diagnostics_for(
        r#"<?php
            namespace Bar {
                function zed(): int { return 1; }
            }
            namespace Foo {
                use function Bar\zed;
                function run(): void {
                    echo zed;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "constant.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn platform_dependent_predefined_constants_get_their_general_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function run(): void {
                assertType("'apache'|'apache2handler'|'cgi'|'cli'|'cli-server'|'embed'|'fpm-fcgi'|'litespeed'|'phpdbg'|non-falsy-string", PHP_SAPI);
                assertType("'Windows'|'BSD'|'Darwin'|'Solaris'|'Linux'|'Unknown'", PHP_OS_FAMILY);
                assertType('int<50207, max>', PHP_VERSION_ID);
                assertType('4|8', PHP_INT_SIZE);
                assertType('resource', STDERR);
                if (PHP_SAPI !== 'cli') {
                    echo 'web';
                }
                if (PHP_EOL === "\n") {
                    echo 'unix';
                }
                if (DIRECTORY_SEPARATOR === '\\') {
                    echo 'windows';
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
