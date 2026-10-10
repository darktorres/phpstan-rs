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

use crate::registry::bodies::MENTIONS_PROPERTY_WRITER;

#[test]
fn a_file_naming_a_property_writer_has_its_bit_as_a_name_or_as_a_string() {
    for source in [
        "<?php (new ReflectionProperty($o, 'p'))->setValue($o, 1);",
        "<?php $x = unserialize($s);",
        "<?php $rows = $statement->fetchAll(PDO::FETCH_CLASS, Row::class);",
        "<?php $rows = array_map('unserialize', $rows);",
        "<?php $rows = array_map(\"Unserialize\", $rows);",
        "<?php $f = 'mysqli_fetch_object';",
        "<?php $o = new ArrayObject($subject);",
    ] {
        assert_ne!(
            facts_of(source).mentions & MENTIONS_PROPERTY_WRITER,
            0,
            "{source}"
        );
    }
    for source in [
        "<?php $x = serialize($s); $y = 'unserializer'; $z = \"set value\";",
        "<?php class A { function getValue() { return 1; } }",
    ] {
        assert_eq!(
            facts_of(source).mentions & MENTIONS_PROPERTY_WRITER,
            0,
            "{source}"
        );
    }
}

#[test]
fn every_property_writer_spelling_names_something_the_stubs_declare() {
    let base = super::super::Project::plain_base();
    for &(spelling, _) in crate::registry::bodies::PROPERTY_WRITER_WATCH {
        let known = base.lookup_function(spelling).is_some()
            || base.classes_by_id().any(|(_, class, reflection)| {
                class.as_str().eq_ignore_ascii_case(spelling)
                    || base.lookup_method(class.as_str(), spelling).is_some()
                    || reflection
                        .consts()
                        .iter()
                        .any(|(name, _)| name.as_str().eq_ignore_ascii_case(spelling))
            });
        assert!(known, "{spelling} is declared by no stub");
    }
}

#[test]
fn every_stub_function_that_fills_an_object_from_data_is_a_property_writer_spelling() {
    let base = super::super::Project::plain_base();
    let spelled: Vec<&str> = crate::registry::bodies::PROPERTY_WRITER_WATCH
        .iter()
        .map(|(spelling, _)| *spelling)
        .collect();
    let mut missing: Vec<String> = Vec::new();
    for (_, name, _) in base.functions_by_id() {
        let lower = name.as_str().to_ascii_lowercase();
        let short = lower.rsplit('\\').next().unwrap_or(&lower);
        if (short.contains("unserialize") || short.contains("fetch_object"))
            && !spelled.contains(&short)
        {
            missing.push(short.to_owned());
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "these fill an object and are not property writer spellings: {missing:?}"
    );
}

#[test]
fn every_stub_method_that_fills_an_object_from_data_is_a_property_writer_spelling() {
    let base = super::super::Project::plain_base();
    let spelled: Vec<&str> = crate::registry::bodies::PROPERTY_WRITER_WATCH
        .iter()
        .map(|(spelling, _)| *spelling)
        .collect();
    let mut missing: Vec<String> = Vec::new();
    for (_, class, reflection) in base.classes_by_id() {
        for name in reflection.methods().keys() {
            let lower = name.as_str().to_ascii_lowercase();
            let fills = lower.contains("unserialize")
                || lower.contains("fetch_object")
                || lower.contains("fetchobject")
                || lower.contains("setfetchmode")
                || lower.contains("exchangearray");
            let hook = matches!(lower.as_str(), "__unserialize" | "bsonunserialize");
            if fills
                && !hook
                && !spelled.contains(&lower.as_str())
                && !spelled.contains(&class.as_str().to_ascii_lowercase().as_str())
            {
                missing.push(format!("{class}::{lower}"));
            }
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "these fill an object and are not property writer spellings: {missing:?}"
    );
}

#[test]
fn a_string_naming_a_writer_is_noted_through_escapes_and_a_class_prefix() {
    for source in [
        r#"<?php $f = 'Foo\\Bar::unserialize';"#,
        r#"<?php $f = "Foo\\Bar::setValue";"#,
        r#"<?php $f = "set\x56alue";"#,
        r#"<?php $f = "\u{73}etValue";"#,
        r#"<?php $f = 'Foo\Bar::setValue';"#,
        r#"<?php $f = 'App\unserialize';"#,
        r#"<?php $f = "\165nserialize";"#,
    ] {
        assert_ne!(
            facts_of(source).mentions & MENTIONS_PROPERTY_WRITER,
            0,
            "{source}"
        );
    }
}
