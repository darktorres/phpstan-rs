//! A pass that a file's facts let the checker skip still reports on a file
//! that has what it looks for, and a file without it reports what the
//! other passes find.
use super::helpers::all_diagnostics_for_with;
use crate::checker::settings::CheckSettings;

fn identifiers(src: &str, configure: impl FnOnce(&mut CheckSettings)) -> Vec<String> {
    all_diagnostics_for_with(src, configure)
        .iter()
        .map(|d| d.identifier.as_str().to_string())
        .collect()
}

fn has(src: &str, identifier: &str) -> bool {
    identifiers(src, |_| {}).iter().any(|i| i == identifier)
}

#[test]
fn a_magic_constant_in_a_closure_is_not_out_of_function() {
    assert!(!has(
        "<?php\n$f = function () { return __FUNCTION__; };\n",
        "magicConstant.outOfFunction"
    ));
    assert!(has(
        "<?php\necho __FUNCTION__;\n",
        "magicConstant.outOfFunction"
    ));
    assert!(has("<?php\necho __CLASS__;\n", "magicConstant.outOfClass"));
}

#[test]
fn a_function_like_is_checked_where_it_is_nested_and_where_it_is_not() {
    let duplicate = "parameter.duplicate";
    assert!(has("<?php\nfunction f($a, $a) {}\n", duplicate));
    assert!(has("<?php\n$g = function ($a, $a) {};\n", duplicate));
    assert!(has(
        "<?php\nfunction f() { return function ($a, $a) {}; }\n",
        duplicate
    ));
    assert!(has(
        "<?php\nclass A { function m() { return fn($a, $a) => 1; } }\n",
        duplicate
    ));
    assert!(has(
        "<?php\nfunction f() { function g($a, $a) {} }\n",
        duplicate
    ));
    assert!(has(
        "<?php\nfunction f() { return new class { function m($a, $a) {} }; }\n",
        duplicate
    ));
    assert!(!has(
        "<?php\nfunction f($a, $b) {}\nclass A { function m($a) {} }\n",
        duplicate
    ));
}

#[test]
fn a_private_method_used_through_an_alias_is_used() {
    let unused = "method.unused";
    assert!(!has(
        "<?php\nclass A {\n    private function p(): void {}\n    public static function make(): void { $o = new self(); $o->p(); }\n}\n",
        unused
    ));
    assert!(has(
        "<?php\nclass A {\n    private function p(): void {}\n    public static function make(): void { $o = new self(); }\n}\n",
        unused
    ));
    assert!(!has(
        "<?php\nclass A {\n    private function p(): void {}\n    public function make(object $o): void { if ($o instanceof self) { $o->p(); } }\n}\n",
        unused
    ));
}

#[test]
fn an_implicit_array_creation_is_reported_whether_or_not_the_body_is_scanned_early() {
    let on = |s: &mut CheckSettings| s.strict_rules.check_implicit_array_creation = true;
    let reports = |src: &str| {
        identifiers(src, on)
            .iter()
            .any(|i| i == "variable.implicitArray")
    };
    assert!(reports("<?php\nfunction f(): void { $a[] = 1; }\n"));
    assert!(reports(
        "<?php\nfunction f(bool $c): void { if ($c) { $a = []; } $a[] = 1; }\n"
    ));
    assert!(reports("<?php\n$a[] = 1;\n"));
    assert!(!reports(
        "<?php\nfunction f(): void { $a = []; $a[] = 1; }\n"
    ));
    assert!(!reports("<?php\nfunction f(): void { $a = 1; }\n"));
}

#[test]
fn a_generator_is_checked_against_its_return_type() {
    let ids = identifiers("<?php\nfunction g(): int { yield 1; }\n", |_| {});
    assert!(ids.iter().any(|i| i.starts_with("generator")), "{ids:?}");
    assert!(
        identifiers("<?php\nfunction h(): int { return 1; }\n", |_| {})
            .iter()
            .all(|i| !i.starts_with("generator"))
    );
}

#[test]
fn a_global_statement_makes_a_call_write_the_global() {
    let with = crate::checker::tests::helpers::type_dump_for(
        "<?php\nfunction f(): void { global $g; $g = 1; }\n$g = 'a';\nf();\n$x = $g;\n",
    );
    let without = crate::checker::tests::helpers::type_dump_for(
        "<?php\nfunction f(): void { $g = 1; }\n$g = 'a';\nf();\n$x = $g;\n",
    );
    assert_ne!(with, without, "{with:?}");
}

fn api_identifiers(files: &[(&str, &str)], checked: &str) -> Vec<String> {
    let mut set = crate::project::test_support::Files::new();
    for (path, source) in files {
        set = set.analysed(path, source);
    }
    let mut project = set.load();
    project.check_settings_mut().phpstan_api = true;
    project
        .check_file(std::path::Path::new(checked))
        .iter()
        .map(|d| d.identifier.as_str().to_string())
        .collect()
}

#[test]
fn a_static_call_is_checked_against_the_api_in_a_file_that_never_spells_phpstan() {
    let base =
        "<?php\nnamespace PHPStan\\Rules;\nclass Base { public static function run(): void {} }\n";
    let mid = "<?php\nnamespace App;\nclass Mid extends \\PHPStan\\Rules\\Base {}\n";
    let user = "<?php\nnamespace App;\nfunction f(): void { Mid::run(); }\n";
    let reported = api_identifiers(
        &[("base.php", base), ("mid.php", mid), ("user.php", user)],
        "user.php",
    );
    let plain = "<?php\nnamespace App;\nclass Other { public static function run(): void {} }\nfunction g(): void { Other::run(); }\n";
    let quiet = api_identifiers(&[("plain.php", plain)], "plain.php");
    assert!(
        reported.iter().any(|i| i.starts_with("phpstanApi")),
        "{reported:?}"
    );
    assert!(
        quiet.iter().all(|i| !i.starts_with("phpstanApi")),
        "{quiet:?}"
    );
}
