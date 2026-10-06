use super::super::*;
use crate::lexer::Lexer;

#[test]
fn parser_identifiers_reuse_the_files_distinct_name_id() {
    let mut parser = Parser::from_lexer(Lexer::new("$item $item $other"));
    let first = parser.expect_variable_spanned().unwrap();
    let repeated = parser.expect_variable_spanned().unwrap();
    let other = parser.expect_variable_spanned().unwrap();

    let first_id = first
        .name_id
        .expect("lexer-backed identifiers have a NameId");
    assert_eq!(first_id, repeated.name_id.unwrap());
    assert_ne!(first_id, other.name_id.unwrap());
    assert_eq!(u32::from(first_id.into_raw()), 0);
    assert_eq!(u32::from(other.name_id.unwrap().into_raw()), 1);
}

#[test]
fn parsed_file_resolves_identifier_ids_and_source_spans() {
    let parsed = crate::parse("<?php $object->member;").unwrap();
    let StmtKind::Expression(expression) = &parsed.stmts[0].kind else {
        panic!("expected an expression statement");
    };
    let ExprKind::PropertyAccess(_, member, _) = &expression.kind else {
        panic!("expected a property access");
    };
    let id = member
        .name_id
        .expect("parsed identifier has a file-local id");

    assert_eq!(parsed.name(id).map(Name::as_str), Some("member"));
    assert_eq!(parsed.source_slice(member.span), Some(&b"member"[..]));
}
