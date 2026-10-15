//! A body that calls `extract()` or `eval()` drops every diagnostic made
//! since it began, the ones of the functions declared in it included, and
//! the bodies after it report as before.

use super::helpers::all_diagnostics_for;

fn count(src: &str) -> usize {
    all_diagnostics_for(src).len()
}

#[test]
fn a_body_without_extract_reports_what_it_finds() {
    assert!(count("<?php function f(): int { return $undefined; }") > 0);
    assert!(count("<?php function f(): void { nope(); }") > 0);
}

#[test]
fn a_body_that_calls_extract_reports_nothing() {
    assert_eq!(
        count("<?php function f(int $d): int { extract([$d]); nope(); return $undefined; }"),
        0
    );
    assert_eq!(
        count("<?php function f(int $d): int { nope(); extract([$d]); return $undefined; }"),
        0
    );
}

#[test]
fn a_body_that_calls_eval_reports_nothing() {
    assert_eq!(
        count("<?php function f(string $c): int { eval($c); nope(); return $undefined; }"),
        0
    );
}

#[test]
fn a_function_declared_in_such_a_body_reports_nothing() {
    let after =
        "<?php function f(int $d): void { extract([$d]); function g(): int { return $u; } }";
    let before =
        "<?php function f(int $d): void { function g(): int { return $u; } extract([$d]); }";
    let alone = "<?php function g(): int { return $u; }";
    assert!(count(alone) > 0);
    assert_eq!(count(after), 0);
    assert_eq!(count(before), 0);
}

#[test]
fn the_body_after_one_with_extract_reports_as_before() {
    let src = "<?php function a(int $d): int { extract([$d]); return $x; }\n\
               function b(): int { return $undefined; }";
    let b_alone = "<?php function b(): int { return $undefined; }";
    assert!(count(b_alone) > 0);
    assert_eq!(count(src), count(b_alone));
}

#[test]
fn a_script_that_calls_extract_reports_nothing() {
    let src = "<?php extract($_GET); function f(): int { return $u; } nope(); echo $x;";
    assert_eq!(count(src), 0);
}

#[test]
fn a_class_method_before_a_top_level_extract_reports_nothing() {
    let src = "<?php class C { public function m(): int { return $u; } }\n\
               extract($_GET);";
    assert_eq!(count(src), 0);
}
