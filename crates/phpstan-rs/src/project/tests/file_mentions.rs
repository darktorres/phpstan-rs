//! The facts a file's parse notes, which let a pass skip a file it cannot
//! find anything in.
use crate::checker::global_constants::MENTIONS_MAGIC;

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
