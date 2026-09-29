//! `registry::name_anonymous_classes`'s naming formula - see that
//! module's doc comment. Unit-level (no `Project`/`Checker` involved) so
//! the 1-based, document-order `line_index` rule (present only when more
//! than one anonymous class shares a line) is tested directly, independent
//! of the end-to-end `project::tests::anonymous_class_naming` suite.

use crate::registry::{Origin, anonymous_class_decls, name_anonymous_classes};
use syntax::ast::{Expr, ExprKind, Stmt};
use syntax::lexer::Lexer;
use syntax::parser::Parser;
use syntax::visit::{Visitor, walk_expr};

fn parse(src: &str) -> Vec<Stmt> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let mut program = Parser::new(tokens).parse_program().unwrap();
    name_anonymous_classes(&mut program, "src/Foo.php");
    program
}

/// Every anonymous class's name, in source order.
fn names(program: &[Stmt]) -> Vec<Option<String>> {
    struct Names(Vec<Option<String>>);
    impl<'a> Visitor<'a> for Names {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if let ExprKind::NewAnonymousClass(class) = &expr.kind {
                self.0
                    .push(class.name.as_ref().map(|name| name.to_string()));
            }
            walk_expr(self, expr);
        }
    }
    let mut names = Names(Vec::new());
    for stmt in program {
        names.visit_stmt(stmt);
    }
    names.0
}

#[test]
fn a_single_anonymous_class_on_its_own_line_gets_no_line_index() {
    let program = parse("<?php $x = new class {};");
    assert_eq!(
        names(&program),
        [Some("class@anonymous/src/Foo.php:1".to_string())]
    );
    assert_eq!(anonymous_class_decls(&program, &Origin::Unknown).len(), 1);
}

#[test]
fn two_anonymous_classes_sharing_a_line_get_document_order_line_indices() {
    let program = parse("<?php $pair = [new class {}, new class {}];");
    assert_eq!(
        names(&program),
        [
            Some("class@anonymous/src/Foo.php:1:1".to_string()),
            Some("class@anonymous/src/Foo.php:1:2".to_string()),
        ]
    );
    assert_eq!(anonymous_class_decls(&program, &Origin::Unknown).len(), 2);
}

#[test]
fn an_anonymous_class_nested_inside_a_method_body_is_still_found() {
    // Unlike `registry::build_declarations` (top-level declarations
    // only), this must descend into method bodies - the overwhelmingly
    // common real-world position for `new class { ... }` (a test double
    // built inside a test method).
    let program = parse(
        "<?php
        class Fixture {
            public function make() {
                return new class {};
            }
        }",
    );
    assert_eq!(
        names(&program),
        [Some("class@anonymous/src/Foo.php:4".to_string())]
    );
    assert_eq!(anonymous_class_decls(&program, &Origin::Unknown).len(), 1);
}
