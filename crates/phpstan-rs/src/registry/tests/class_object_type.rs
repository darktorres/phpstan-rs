//! A class keeps the plain object type that names it.

use super::super::*;
use syntax::lexer::Lexer;
use syntax::parser::Parser;
use types::Type;

fn index(src: &str) -> Registry {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = Registry::default();
    registry.index(&program, &Origin::Unknown, false);
    registry
}

#[test]
fn a_class_gives_the_same_object_type_each_time() {
    let r = index("<?php class Foo {}");
    let (declared, class) = r.lookup_class("foo").unwrap();
    let first = class.object_type(declared);
    assert_eq!(first, Type::object("Foo".into(), Vec::new()));
    assert_eq!(class.object_type(declared), first);
}

#[test]
fn a_copy_of_a_class_under_another_name_gives_that_names_type() {
    let r = index("<?php class Foo {}");
    let (declared, class) = r.lookup_class("Foo").unwrap();
    class.object_type(declared);
    // The kept type is of `Foo`; asked for another name, the class gives
    // that name's type and keeps the first.
    assert_eq!(
        class.object_type("Bar"),
        Type::object("Bar".into(), Vec::new())
    );
    assert_eq!(
        class.object_type("Foo"),
        Type::object("Foo".into(), Vec::new())
    );
}

#[test]
fn the_kept_type_is_no_part_of_the_class_value() {
    let r = index("<?php class Foo {}");
    let (declared, class) = r.lookup_class("Foo").unwrap();
    let fresh = class.clone();
    class.object_type(declared);
    assert_eq!(*class, fresh);
}
