//! The facts a file's parse notes, which let a pass skip a file it cannot
//! find anything in.
use crate::checker::global_constants::MENTIONS_MAGIC;
use crate::checker::phpstan_api::MENTIONS_PHPSTAN;

fn facts_of(source: &str) -> syntax::FileFacts {
    syntax::parse_watching(source, None, Some(&super::super::WATCHED))
        .expect("the file parses")
        .facts
}

#[test]
fn a_file_naming_a_magic_constant_has_its_bit_in_any_case() {
    assert_ne!(
        facts_of("<?php function f() { return __FUNCTION__; }").mentions & MENTIONS_MAGIC,
        0
    );
    assert_ne!(
        facts_of("<?php class A { function f() { return __Method__; } }").mentions & MENTIONS_MAGIC,
        0
    );
    assert_eq!(
        facts_of("<?php function f() { return __CLASS__; }").mentions & MENTIONS_MAGIC,
        0
    );
}

#[test]
fn a_file_naming_phpstan_has_its_bit_wherever_the_name_is_spelled() {
    for source in [
        "<?php namespace PHPStan\\Rules;",
        "<?php namespace phpstan;",
        "<?php use PHPStan\\Rules\\Rule; class A implements Rule {}",
        "<?php use PHPStan\\Rules\\{Rule, RuleError};",
        "<?php $x = new \\PHPStan\\Type\\ObjectType('A');",
        "<?php $x instanceof \\phpstan\\Type\\Type;",
    ] {
        assert_ne!(facts_of(source).mentions & MENTIONS_PHPSTAN, 0, "{source}");
    }
    assert_eq!(
        facts_of("<?php namespace App; class A { function f() { return A::g(); } }").mentions
            & MENTIONS_PHPSTAN,
        0
    );
}
