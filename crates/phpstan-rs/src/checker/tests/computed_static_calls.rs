//! A call or constant fetch whose member name is a known string (`Foo::{'run'}()`)
//! is checked as the call or fetch it names: it throws what that member throws, and
//! its name is the source's own.

use super::helpers::type_dump_for;

/// The type dump's line for `line`, which must be present.
fn at(dump: &[String], line: u32) -> &str {
    dump.iter()
        .find(|l| l.starts_with(&format!("{line}:")))
        .unwrap_or_else(|| panic!("no dump for line {line}: {dump:#?}"))
}

/// `call` is the text of the call in `$x`'s try body, and the dump's line for
/// the statement after the try.
fn after_try(call: &str) -> String {
    let src = format!(
        "<?php\nclass Foo {{\n    /** @throws \\RuntimeException */\n    public static function run(): void {{\n        throw new \\RuntimeException('x');\n    }}\n}}\nfunction f(): void {{\n    $x = 1;\n    try {{\n        {call};\n    }} catch (\\LogicException $e) {{\n        $x = 'a';\n    }}\n    $y = $x;\n}}\n"
    );
    let dump = type_dump_for(&src);
    at(&dump, 15).to_string()
}

#[test]
fn a_computed_static_call_throws_what_its_method_throws_as_a_direct_call_does() {
    let direct = after_try("Foo::run()");
    let computed = after_try("Foo::{'run'}()");
    assert_eq!(computed, direct);
    assert!(!computed.contains("string"), "{computed}");
}
