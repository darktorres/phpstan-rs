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
    assert!(c.def.methods[0].function.doc_comment.is_some());
    assert!(c.def.methods[1].function.doc_comment.is_none());
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
        c.def.properties[0]
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
fn deeply_nested_expressions_parse_on_a_default_thread() {
    // php-src's Zend/tests/stack_limit/stack_limit_013.phpt nests about
    // 7,700 unary operators; parentheses, arrays and blocks nest the same
    // way. Each parses on a test thread's ordinary stack.
    for (open, close) in [("~", ""), ("(", ")"), ("[", "]")] {
        let source = format!(
            "<?php $x = {}1{};",
            open.repeat(20_000),
            close.repeat(20_000)
        );
        Parser::new(crate::lexer::Lexer::new(&source).tokenize_with_spans())
            .parse_program()
            .expect("deep nesting parses");
    }
    let blocks = format!("<?php {}echo 1;{}", "{".repeat(20_000), "}".repeat(20_000));
    Parser::new(crate::lexer::Lexer::new(&blocks).tokenize_with_spans())
        .parse_program()
        .expect("deep blocks parse");
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
