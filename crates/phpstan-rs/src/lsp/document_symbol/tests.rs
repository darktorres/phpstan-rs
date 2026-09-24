use super::document_symbols;
use crate::lexer::Lexer;
use crate::parser::Parser;
use lsp_types::SymbolKind;

fn parse(src: &str) -> Vec<crate::ast::Stmt> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    Parser::new(tokens).parse_program().unwrap()
}

#[test]
fn a_top_level_function_becomes_a_function_symbol() {
    let stmts = parse("<?php function foo() {}");
    let symbols = document_symbols(&stmts);
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "foo");
    assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
    assert!(symbols[0].children.is_none());
}

#[test]
fn a_class_nests_its_methods_properties_and_consts() {
    let stmts = parse(
        r#"<?php
        class Foo {
            const BAR = 1;
            public int $x;
            public function m(): void {}
        }
        "#,
    );
    let symbols = document_symbols(&stmts);
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "Foo");
    assert_eq!(symbols[0].kind, SymbolKind::CLASS);
    let children = symbols[0].children.as_ref().unwrap();
    let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["BAR", "x", "m"]);
    assert_eq!(children[0].kind, SymbolKind::CONSTANT);
    assert_eq!(children[1].kind, SymbolKind::PROPERTY);
    assert_eq!(children[2].kind, SymbolKind::METHOD);
}

#[test]
fn an_interface_reports_the_interface_kind() {
    let stmts = parse("<?php interface Foo { public function m(): void; }");
    let symbols = document_symbols(&stmts);
    assert_eq!(symbols[0].kind, SymbolKind::INTERFACE);
}

#[test]
fn an_enum_nests_its_cases() {
    let stmts = parse(
        r#"<?php
        enum Suit: string {
            case Hearts = 'H';
            case Spades = 'S';
        }
        "#,
    );
    let symbols = document_symbols(&stmts);
    assert_eq!(symbols[0].kind, SymbolKind::ENUM);
    let children = symbols[0].children.as_ref().unwrap();
    let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Hearts", "Spades"]);
    assert!(children.iter().all(|c| c.kind == SymbolKind::ENUM_MEMBER));
}

#[test]
fn a_declaration_inside_a_top_level_if_is_still_reported() {
    let stmts = parse(
        r#"<?php
        if (true) {
            function foo() {}
        }
        "#,
    );
    let symbols = document_symbols(&stmts);
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "foo");
}
