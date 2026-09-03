//! Doc-comment attachment and parser robustness under pathological or malformed input.

use super::super::*;
use super::helpers::*;

#[test]
fn doc_comment_attaches_to_top_level_function() {
    let program = parse(
        r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double(int $x): int { return $x * 2; }
        "#,
    );
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert!(f.doc_comment.as_ref().unwrap().contains("@return int"));
}

#[test]
fn doc_comment_attaches_to_method_not_the_previous_one() {
    let program = parse(
        r#"<?php
            class Box {
                /** @return int */
                public function a(): int { return 1; }

                public function b(): int { return 2; }
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert!(c.methods[0].function.doc_comment.is_some());
    assert!(c.methods[1].function.doc_comment.is_none());
}

#[test]
fn doc_comment_attaches_to_property() {
    let program = parse(
        r#"<?php
            class Box {
                /** @var int */
                public $value;
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert!(
        c.properties[0]
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("@var int")
    );
}

#[test]
fn empty_slash_star_star_slash_is_not_treated_as_a_doc_comment() {
    // `/**/` is the classic ambiguous case: it looks like the start of
    // a doc comment but is conventionally just an empty regular one.
    let program = parse(
        r#"<?php
            /**/
            function f(): int { return 1; }
        "#,
    );
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert!(f.doc_comment.is_none());
}

#[test]
fn regular_comment_is_not_captured_as_a_doc_comment() {
    let program = parse(
        r#"<?php
            // just a regular comment
            function f(): int { return 1; }
        "#,
    );
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert!(f.doc_comment.is_none());
}

#[test]
fn moderately_nested_unary_still_parses() {
    let nested = "~".repeat(500);
    parse(&format!("<?php $x = {nested}1;"));
}

#[test]
#[should_panic(expected = "expression nested too deeply")]
fn pathologically_nested_unary_fails_cleanly_instead_of_overflowing_the_stack() {
    // php-src's Zend/tests/stack_limit/stack_limit_013.phpt is exactly
    // this shape (~7700 levels) - it's designed to test the real
    // engine's own stack-depth guard, and used to take this parser's
    // process down with an uncatchable stack overflow instead of a
    // normal parse error.
    let nested = "~".repeat(8000);
    parse(&format!("<?php $x = {nested}1;"));
}

#[test]
fn halt_compiler_stops_parsing_cleanly() {
    let program = parse(
        r#"<?php
            print "yo!\n";
            __HALT_COMPILER();
            none of this should be parsed at all! {{{ ???
        "#,
    );
    assert_eq!(program.len(), 2);
}
