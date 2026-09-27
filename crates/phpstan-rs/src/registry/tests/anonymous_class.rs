//! `registry::collect_anonymous_classes`'s own naming formula - see that
//! module's doc comment. Unit-level (no `Project`/`Checker` involved) so
//! the 1-based, document-order `line_index` rule (present only when more
//! than one anonymous class shares a line) is tested directly, independent
//! of the end-to-end `project::tests::anonymous_class_naming` suite.

use crate::registry::anonymous_class::synthetic_name;
use crate::registry::{Origin, collect_anonymous_classes};
use syntax::lexer::Lexer;
use syntax::parser::Parser;

fn parse(src: &str) -> Vec<syntax::ast::Stmt> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    Parser::new(tokens).parse_program().unwrap()
}

#[test]
fn a_single_anonymous_class_on_its_own_line_gets_no_line_index() {
    let program = parse("<?php $x = new class {};");
    let (decls, names) = collect_anonymous_classes(&program, "src/Foo.php", &Origin::Unknown);
    assert_eq!(decls.len(), 1);
    assert_eq!(names.len(), 1);
    let name = names.values().next().unwrap();
    assert_eq!(name, &synthetic_name("src/Foo.php", 1, None));
}

#[test]
fn two_anonymous_classes_sharing_a_line_get_document_order_line_indices() {
    let program = parse("<?php $pair = [new class {}, new class {}];");
    let (decls, names) = collect_anonymous_classes(&program, "src/Foo.php", &Origin::Unknown);
    assert_eq!(decls.len(), 2);
    let mut ordered: Vec<_> = names.into_iter().collect();
    ordered.sort_by_key(|(span, _)| span.start.col);
    assert_eq!(ordered[0].1, synthetic_name("src/Foo.php", 1, Some(1)));
    assert_eq!(ordered[1].1, synthetic_name("src/Foo.php", 1, Some(2)));
}

#[test]
fn an_anonymous_class_nested_inside_a_method_body_is_still_found() {
    // Unlike `registry::build_declarations` (top-level declarations
    // only), this collector must descend into method bodies - the
    // overwhelmingly common real-world position for `new class { ... }`
    // (a test double built inside a test method).
    let program = parse(
        "<?php
        class Fixture {
            public function make() {
                return new class {};
            }
        }",
    );
    let (decls, names) = collect_anonymous_classes(&program, "src/Foo.php", &Origin::Unknown);
    assert_eq!(decls.len(), 1);
    assert_eq!(names.len(), 1);
}
