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
fn pathologically_nested_unary_fails_cleanly_instead_of_overflowing_the_stack() {
    // php-src's Zend/tests/stack_limit/stack_limit_013.phpt is exactly
    // this shape (~7700 levels); parsing it must produce a parse error,
    // not a stack overflow. Runs on a thread with a generous stack, as the
    // checker's workers do: a debug build's frames are large enough to
    // exhaust a default thread stack before the depth guard trips.
    let nested = "~".repeat(8000);
    let source = format!("<?php $x = {nested}1;");
    let result = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            Parser::new(crate::lexer::Lexer::new(&source).tokenize_with_spans())
                .parse_program()
                .map(|_| ())
        })
        .expect("failed to spawn worker thread")
        .join()
        .expect("the parser must not panic");
    let error = result.expect_err("expected a parse error");
    assert!(
        error.message.contains("expression nested too deeply"),
        "got: {}",
        error.message
    );
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
