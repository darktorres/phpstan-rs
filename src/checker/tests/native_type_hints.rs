//! PHPDoc-only types that can be declared natively.

use super::helpers::*;

fn found(src: &str, version: Option<(u8, u8)>) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, |r| {
        r.check_native_type_hints = true;
        r.php_version = version;
    })
    .into_iter()
    .filter(|d| d.identifier.starts_with("missingNativeType."))
    .map(|d| format!("{} {}", d.span.start.line, d.message))
    .collect();
    v.sort();
    v
}

#[test]
fn functions_by_version() {
    let src = r#"<?php
/**
 * @param int $a
 * @param list<string> $b
 * @param int|string $c
 * @param int $d
 * @param resource $e
 * @param positive-int $f
 * @return void
 */
function f($a, $b, $c, $d = null, $e = null, $f = 1) {}
/** @return int|false */
function g() { return 1; }
/** @return mixed */
function h() { return 1; }
/** @param int $x */
function typed(int $x): void {}
/** @param int $x */
function badDefault($x = 'a'): void {}
"#;
    assert_eq!(
        found(src, Some((8, 2))),
        vec![
            "11 Function f() parameter $a can be declared natively as int.",
            "11 Function f() parameter $b can be declared natively as array.",
            "11 Function f() parameter $c can be declared natively as int|string.",
            "11 Function f() parameter $d can be declared natively as ?int.",
            "11 Function f() parameter $f can be declared natively as int.",
            "11 Function f() return type can be declared natively as void.",
            "13 Function g() return type can be declared natively as int|false.",
            "15 Function h() return type can be declared natively as mixed.",
        ]
    );
    assert_eq!(
        found(src, None),
        vec![
            "11 Function f() parameter $a can be declared natively as int.",
            "11 Function f() parameter $b can be declared natively as array.",
            "11 Function f() parameter $d can be declared natively as ?int.",
            "11 Function f() parameter $f can be declared natively as int.",
            "11 Function f() return type can be declared natively as void.",
        ]
    );
}

#[test]
fn inheritance_constraints() {
    let src = r#"<?php
class Base {
    /**
     * @param int $x
     * @return int
     */
    public function untypedParam($x) { return 1; }
    public function typedParam(array $x): array { return []; }
    /** @return string */
    public function overriddenUntyped() { return ''; }
    /** @return string */
    public function overriddenTyped() { return ''; }
    /** @return static */
    public function fluent() { return $this; }
    /** @return string */
    public function __toString() { return ''; }
}
class Child extends Base {
    /** @param int $x */
    public function untypedParam($x): int { return 1; }
    /** @param list<int> $x */
    public function typedParam($x): array { return []; }
    public function overriddenUntyped() { return ''; }
    public function overriddenTyped(): string { return ''; }
}
"#;
    assert_eq!(
        found(src, Some((8, 3))),
        vec![
            "12 Method Base::overriddenTyped() return type can be declared natively as string.",
            "14 Method Base::fluent() return type can be declared natively as static.",
            "22 Method Child::typedParam() parameter $x can be declared natively as array.",
            "7 Method Base::untypedParam() parameter $x can be declared natively as int.",
            "7 Method Base::untypedParam() return type can be declared natively as int.",
        ]
    );
}

#[test]
fn closure_parameters_from_the_callee() {
    let src = r#"<?php
/** @param list<int> $ints */
function f(array $ints, array $unknown): void {
    array_map(fn ($i) => $i * 2, $ints);
    array_map(function ($i) { return $i; }, $ints);
    array_map(fn (int $i) => $i, $ints);
    array_map(fn ($x) => $x, $unknown);
}
"#;
    assert_eq!(
        found(src, Some((8, 3))),
        vec![
            "4 Parameter $i of the closure can be declared natively as int.",
            "5 Parameter $i of the closure can be declared natively as int.",
        ]
    );
}
