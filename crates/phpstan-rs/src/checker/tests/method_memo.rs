//! `Checker::lookup_method_with_owner` answers as the registry does, the
//! first time a class and method are asked for and every time after.
use super::super::*;

#[test]
fn a_method_asked_for_again_is_the_one_the_registry_gives() {
    let tokens = syntax::lexer::Lexer::new(
        "<?php interface I { public function viaInterface(): int; } \
         class A implements I { public function viaInterface(): int { return 1; } public function own(): int { return 1; } } \
         class B extends A { public function __construct() {} }",
    )
    .tokenize_with_spans();
    let program = syntax::parser::Parser::new(tokens).parse_program().unwrap();
    let registry = Registry::build(&program);
    let checker = Checker::new(&registry);
    let asked = [
        ("A", "own"),
        ("a", "OWN"),
        ("B", "own"),
        ("B", "viaInterface"),
        ("I", "viaInterface"),
        ("B", "__construct"),
        ("A", "missing"),
        ("Unknown", "own"),
        ("", ""),
    ];
    for _ in 0..3 {
        for (class, method) in asked {
            let expected = registry.lookup_method_with_owner(class, method);
            let got = checker.lookup_method_with_owner(class, method);
            assert_eq!(
                got.as_ref()
                    .map(|(owner, sig)| (owner.clone(), std::ptr::from_ref(*sig))),
                expected
                    .as_ref()
                    .map(|(owner, sig)| (owner.clone(), std::ptr::from_ref(*sig))),
                "{class}::{method}"
            );
        }
    }
}
