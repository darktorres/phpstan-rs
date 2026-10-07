//! Doc-comment attachment and parser robustness under pathological or malformed input.

use super::super::*;
use super::helpers::*;

#[test]
fn inline_html_ast_payload_is_a_slice_of_the_retained_source() {
    let parsed = crate::parse::parse("<title><?= $title ?></title>").unwrap();
    let stmt = parsed.stmt(parsed.statements()[0]).unwrap();
    let StmtKind::InlineHtml(span) = &stmt.kind else {
        panic!("expected inline HTML statement: {stmt:?}");
    };
    assert_eq!(parsed.source_slice(*span), Some(&b"<title>"[..]));
}

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
    let f = program.function(*f);
    assert!(
        program
            .doc_comment_text(f.doc_comment.as_ref().unwrap())
            .unwrap()
            .contains("@return int")
    );
}

#[test]
fn grouped_doc_ast_keeps_exact_source_ranges() {
    let src = "<?php /** first */\r\n/** second */ function f() {}";
    let program = parse(src);
    let StmtKind::FunctionDecl(id) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    let doc = program.function(*id).doc_comment.as_ref().unwrap();
    let spans = doc.spans();

    assert_eq!(spans.len(), 2);
    assert_eq!(
        &src.as_bytes()[spans[0].start as usize..spans[0].end as usize],
        b"/** first */"
    );
    assert_eq!(
        &src.as_bytes()[spans[1].start as usize..spans[1].end as usize],
        b"/** second */"
    );
    assert_eq!(
        program.doc_comment_text(doc).as_deref(),
        Some("/** first \n second \n*/")
    );
}

#[test]
fn source_backed_grouped_docs_are_recorded_for_sites_and_frames() {
    let src = "<?php /** first */\r\n/** second */ function f() {}";
    let parsed = crate::parse::parse(src).unwrap();
    let expected = "/** first \n second \n*/";

    assert!(
        parsed
            .references
            .as_ref()
            .unwrap()
            .doc_sites
            .iter()
            .any(|site| site.doc.as_ref() == expected)
    );
    assert!(
        parsed
            .references
            .as_ref()
            .unwrap()
            .doc_frames
            .iter()
            .any(|frame| frame.doc.as_ref() == expected)
    );
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
    let c = program.arenas.class_decl(c.clone());
    let def = program.class_def(c.def);
    let methods = program.methods(def.methods.clone());
    assert!(methods[0].function.doc_comment.is_some());
    assert!(methods[1].function.doc_comment.is_none());
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
    let c = program.arenas.class_decl(c.clone());
    let def = program.class_def(c.def);
    assert!(
        program
            .doc_comment_text(
                def.properties(&program.arenas)[0]
                    .doc_comment
                    .as_ref()
                    .unwrap()
            )
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
    let f = program.function(*f);
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
    let f = program.function(*f);
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
        Parser::from_token_stream(crate::lexer::Lexer::new(&source).lex_token_stream())
            .parse_program()
            .expect("deep nesting parses");
    }
    let blocks = format!("<?php {}echo 1;{}", "{".repeat(20_000), "}".repeat(20_000));
    Parser::from_token_stream(crate::lexer::Lexer::new(&blocks).lex_token_stream())
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
