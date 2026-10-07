//! `registry::name_anonymous_classes`'s naming formula - see that
//! module's doc comment. Unit-level (no `Project`/`Checker` involved) so
//! the 1-based, document-order `line_index` rule (present only when more
//! than one anonymous class shares a line) is tested directly, independent
//! of the end-to-end `project::tests::anonymous_class_naming` suite.

use crate::registry::{Origin, anonymous_class_decls, name_anonymous_classes};
use syntax::ast::*;
use syntax::span::LineIndex;
use syntax::visit::arena::{self, Visitor};

fn parse(src: &str) -> syntax::ParsedFile {
    let mut parsed = syntax::parse(src.as_bytes()).unwrap();
    let statements = parsed.stmts.clone();
    name_anonymous_classes(
        &mut parsed.arenas,
        &statements,
        "src/Foo.php",
        &LineIndex::new(src.as_bytes()),
    );
    parsed
}

/// Every anonymous class's name, in source order.
fn names(parsed: &syntax::ParsedFile) -> Vec<Option<String>> {
    struct Names(Vec<Option<String>>);
    impl Visitor for Names {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if let Some(ExprNode {
                kind: ExprKind::NewAnonymousClass(class_id),
                ..
            }) = arenas.expr(id)
            {
                self.0.push(
                    arenas
                        .anonymous_class_name(*class_id)
                        .map(ToString::to_string),
                );
            }
            arena::walk_expr(self, arenas, id);
        }
    }
    let mut names = Names(Vec::new());
    arena::walk_stmts(&mut names, &parsed.arenas, parsed.statements());
    names.0
}

#[test]
fn names_are_owned_by_the_file_and_cleared_with_its_arenas() {
    let src = "<?php $x = new class {};";
    let mut parsed = syntax::parse(src.as_bytes()).unwrap();
    let mut ids = Vec::new();
    struct Find<'a>(&'a mut Vec<AnonymousClassId>);
    impl Visitor for Find<'_> {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if let Some(ExprNode {
                kind: ExprKind::NewAnonymousClass(class_id),
                ..
            }) = arenas.expr(id)
            {
                self.0.push(*class_id);
            }
            arena::walk_expr(self, arenas, id);
        }
    }
    arena::walk_stmts(&mut Find(&mut ids), &parsed.arenas, parsed.statements());
    assert_eq!(ids.len(), 1);
    assert!(parsed.arenas.anonymous_class_name(ids[0]).is_none());

    let statements = parsed.stmts.clone();
    name_anonymous_classes(
        &mut parsed.arenas,
        &statements,
        "src/Foo.php",
        &LineIndex::new(src.as_bytes()),
    );
    assert_eq!(
        parsed
            .arenas
            .anonymous_class_name(ids[0])
            .map(ToString::to_string),
        Some("class@anonymous/src/Foo.php:1".to_string()),
    );

    parsed.arenas.clear();
    assert!(parsed.arenas.anonymous_class_name(ids[0]).is_none());
}

#[test]
fn transient_parse_returns_and_releases_anonymous_name_side_table_with_its_owner() {
    let src = "<?php $x = new class {};";
    let mut scratch = NodeArenas::default();
    let mut parsed =
        syntax::parse_watching_transient_with_scratch(src.as_bytes(), None, None, &mut scratch)
            .unwrap();
    let mut ids = Vec::new();
    struct Find<'a>(&'a mut Vec<AnonymousClassId>);
    impl Visitor for Find<'_> {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if let Some(ExprNode {
                kind: ExprKind::NewAnonymousClass(class_id),
                ..
            }) = arenas.expr(id)
            {
                self.0.push(*class_id);
            }
            arena::walk_expr(self, arenas, id);
        }
    }
    arena::walk_stmts(&mut Find(&mut ids), &parsed.arenas, parsed.statements());
    let statements = parsed.stmts.clone();
    name_anonymous_classes(
        &mut parsed.arenas,
        &statements,
        "src/Foo.php",
        &LineIndex::new(src.as_bytes()),
    );
    assert_eq!(
        parsed
            .arenas
            .anonymous_class_name(ids[0])
            .map(ToString::to_string),
        Some("class@anonymous/src/Foo.php:1".to_string()),
    );

    let mut returned = parsed.into_node_arenas();
    returned.clear();
    scratch = returned;
    assert!(scratch.anonymous_class_name(ids[0]).is_none());
}

fn decl_count(parsed: &syntax::ParsedFile) -> usize {
    anonymous_class_decls(&parsed.arenas, parsed.statements(), &Origin::Unknown).len()
}

#[test]
fn a_single_anonymous_class_on_its_own_line_gets_no_line_index() {
    let parsed = parse("<?php $x = new class {};");
    assert_eq!(
        names(&parsed),
        [Some("class@anonymous/src/Foo.php:1".to_string())]
    );
    assert_eq!(decl_count(&parsed), 1);
}

#[test]
fn two_anonymous_classes_sharing_a_line_get_document_order_line_indices() {
    let parsed = parse("<?php $pair = [new class {}, new class {}];");
    assert_eq!(
        names(&parsed),
        [
            Some("class@anonymous/src/Foo.php:1:1".to_string()),
            Some("class@anonymous/src/Foo.php:1:2".to_string()),
        ]
    );
    assert_eq!(decl_count(&parsed), 2);
}

#[test]
fn an_anonymous_class_nested_inside_a_method_body_is_still_found() {
    // Unlike `registry::build_declarations` (top-level declarations
    // only), this must descend into method bodies - the overwhelmingly
    // common real-world position for `new class { ... }` (a test double
    // built inside a test method).
    let parsed = parse(
        "<?php
        class Fixture {
            public function make() {
                return new class {};
            }
        }",
    );
    assert_eq!(
        names(&parsed),
        [Some("class@anonymous/src/Foo.php:4".to_string())]
    );
    assert_eq!(decl_count(&parsed), 1);
}
