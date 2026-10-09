//! The catalog call-result memo is local to a file and preserves distinct
//! flow-sensitive argument types at one source span.
use super::super::*;

fn checker() -> Checker<'static> {
    let tokens = syntax::lexer::Lexer::new("<?php").tokenize_with_spans();
    let program = syntax::parser::Parser::new(tokens).parse_program().unwrap();
    let registry = Box::leak(Box::new(Registry::build(
        &program.arenas,
        program.statements(),
    )));
    Checker::new(registry)
}

#[test]
fn same_span_keeps_distinct_argument_states_and_file_change_clears_them() {
    let mut checker = checker();
    let span = syntax::span::Span::default();
    let string_argument = vec![Type::string_literal("abc".into())];
    let int_argument = vec![Type::int_literal(7)];
    let string_result = Type::int_literal(3);
    let int_result = Type::int_range(Some(0), None);
    let string_evaluation = CatalogCallEvaluation {
        return_type: string_result.clone(),
        diagnostics: vec![],
        diagnostics_closed: true,
    };
    let int_evaluation = CatalogCallEvaluation {
        return_type: int_result.clone(),
        diagnostics: vec![],
        diagnostics_closed: true,
    };

    checker.remember_catalog_return_type(span, string_argument.clone(), string_evaluation);
    checker.remember_catalog_return_type(span, int_argument.clone(), int_evaluation);

    assert_eq!(
        checker
            .cached_catalog_return_type(span, &string_argument)
            .map(|evaluation| evaluation.return_type),
        Some(string_result)
    );
    assert_eq!(
        checker
            .cached_catalog_return_type(span, &int_argument)
            .map(|evaluation| evaluation.return_type),
        Some(int_result)
    );

    checker.set_current_file(std::path::Path::new("first.php"));
    assert!(
        checker
            .cached_catalog_return_type(span, &string_argument)
            .is_none()
    );
    assert!(
        checker
            .cached_catalog_return_type(span, &int_argument)
            .is_none()
    );
}
